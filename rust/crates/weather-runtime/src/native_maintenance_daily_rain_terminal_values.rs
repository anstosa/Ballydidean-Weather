//! Post-burn rain confirmation assembly from parser-authenticated cold history.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use weather_contracts::{
    AdjustmentRevisionCommitReceipt, AdjustmentRevisionCommitReceiptProjectionKindEnum,
    AdjustmentRevisionProjectionDocument, FieldPresence, JsonValue, Nullable,
};
use weather_edge::{
    ArchiveWirePrimitives, PrivateShadowArchiveCodec, PrivateShadowCapsuleDocument,
    PrivateShadowReceiptCodec, RevisionArchiveCanonicalCodec,
};

use crate::maintenance_canonical_json::{field, text, valid_hash};
use crate::native_maintenance_daily_terminal_values::{
    NativeMaintenanceDailyDerivedTarget, NativeMaintenanceDailyTerminalChunk,
    NativeMaintenanceDailyTerminalValues, NativeMaintenanceDailyTerminalValuesError,
    NativeMaintenanceDailyTerminalValuesInput,
};
use crate::native_maintenance_policy_comparison::{object, string};
use crate::native_maintenance_retirement_files_v2::format_digest;
use crate::worker_rain_persistence::fixed_gauge_weighted_median;
use crate::{
    ApiMaintenanceRevisionTransportDocument, NativeMaintenanceCaptureFamily,
    NativeMaintenanceDailyTerminalPreparation, NativeMaintenanceDailyTerminalPreparationInput,
    NativeMaintenanceHistoricalArchiveIndex, NativeMaintenanceHistoricalOccurrence,
    NativeModelEvidenceValues, NativePredecessorModelCalendar, NativeShadowProducerCodec,
    NativeWirePrimitives, VerifiedNativeMaintenanceCaptureEpochWitness,
    decode_native_api_revision_document, prepare_native_maintenance_daily_terminal_material,
};

const HOUR: i64 = 3_600_000;
const DAY: i64 = 24 * HOUR;
const CYCLE: i64 = 6 * HOUR;
const MAXIMUM_PART_BYTES: usize = 8 * 1_024 * 1_024;
const CAPSULE_VERSION: &str = "adjustment-shadow-revision-capsule/v2";
const CONTROL_VERSION: &str = "adjustment-rain-gate-control-projection/v3";
const TARGET_VERSION: &str = "adjustment-rain-fixed-gauge-target-projection/v1";
const REVISION_V1_VERSION: &str = "adjustment-revision-projection/v1";
const REVISION_V2_VERSION: &str = "adjustment-revision-batch-projection/v2";

#[derive(Clone)]
struct ExpectedPart {
    capture_local_date: String,
    expected_keys: Vec<String>,
    from_local_date: String,
    logical_chunk_index: usize,
    part_count: usize,
    part_index: usize,
    to_local_date_exclusive: String,
}

#[derive(Clone)]
struct RainCapsule {
    body: JsonValue,
    capsule: PrivateShadowCapsuleDocument,
    comparator: JsonValue,
    comparator_bytes: Vec<u8>,
    graph_manifest_sha256: String,
    payload_bytes: Vec<u8>,
    payload_identity_sha256: String,
    prediction_committed_at: String,
    receipt: AdjustmentRevisionCommitReceipt,
    source: JsonValue,
    source_bytes: Vec<u8>,
}

#[derive(Clone)]
struct RainControl {
    document: JsonValue,
    graph_manifest_sha256: String,
    payload_identity_sha256: String,
    receipt: AdjustmentRevisionCommitReceipt,
}

#[derive(Clone)]
struct RainBestMatch {
    document: JsonValue,
    graph_manifest_sha256: String,
    member_sha256: String,
    payload_identity_sha256: String,
    receipt: AdjustmentRevisionCommitReceipt,
    row: JsonValue,
}

#[derive(Clone)]
struct RainTargetMember {
    member_sha256: String,
    receipt: AdjustmentRevisionCommitReceipt,
}

#[derive(Clone)]
struct RainTarget {
    actual: Option<f64>,
    document: JsonValue,
    graph_manifest_sha256: String,
    maximum_archive_ordinal: String,
    members: Vec<RainTargetMember>,
    payload_identity_sha256: String,
}

struct RainHistoryIndexes {
    actual_best_matches: HashMap<String, Vec<RainBestMatch>>,
    capsules: HashMap<String, RainCapsule>,
    controls: HashMap<String, Vec<RainControl>>,
    targets: HashMap<String, Vec<RainTarget>>,
}

struct RainResolvedRecord {
    graph_binding: JsonValue,
    graph_manifest_sha256s: Vec<String>,
    rain_gate_input_member_sha256: String,
    record: JsonValue,
    required_member_identity_sha256s: Vec<String>,
    target_member_sha256: String,
}

/// Assemble one source-exact complete rain confirmation after a complete burn.
pub fn assemble_native_maintenance_daily_rain_confirmation(
    input: NativeMaintenanceDailyTerminalValuesInput<'_>,
) -> Result<NativeMaintenanceDailyTerminalValues, NativeMaintenanceDailyTerminalValuesError> {
    assemble_rain_confirmation(input, false)
}

/// Assemble one source-exact rain confirmation after an unsupported or complete burn.
pub fn assemble_native_maintenance_daily_rain_unsupported_confirmation(
    input: NativeMaintenanceDailyTerminalValuesInput<'_>,
) -> Result<NativeMaintenanceDailyTerminalValues, NativeMaintenanceDailyTerminalValuesError> {
    assemble_rain_confirmation(input, true)
}

/// Execute the common complete and unsupported rain post-burn path.
fn assemble_rain_confirmation(
    input: NativeMaintenanceDailyTerminalValuesInput<'_>,
    unsupported: bool,
) -> Result<NativeMaintenanceDailyTerminalValues, NativeMaintenanceDailyTerminalValuesError> {
    use NativeMaintenanceDailyTerminalValuesError as Error;

    let wire = NativeWirePrimitives::new();
    let preparation = prepare_native_maintenance_daily_terminal_material(
        NativeMaintenanceDailyTerminalPreparationInput {
            catalog_input_manifest_sha256: input.catalog_input_manifest_sha256,
            clock_at: input.clock_at,
            confirmation_registration: Some(input.confirmation_registration),
            due: input.due,
            epoch_witness: input.epoch_witness,
            history: input.history,
            shadow_registration: input.shadow_registration,
            source_commit: input.source_commit,
            verified_epoch_witness: input.verified_epoch_witness,
        },
    )
    .map_err(|_| Error::Plan)?;
    let current_plan = match &preparation {
        NativeMaintenanceDailyTerminalPreparation::Supported { plan } => plan,
        NativeMaintenanceDailyTerminalPreparation::Unsupported { plan } if unsupported => plan,
        _ => return Err(Error::Plan),
    };
    // bind opening to the exact value-blind plan frozen before burn
    if canonical_bytes(&wire, current_plan)? != canonical_bytes(&wire, input.plan)? {
        return Err(Error::Plan);
    }
    // keep the rain path disjoint from the temperature and wind assembler
    if text(input.confirmation_registration, "family").as_deref() != Some("rain")
        || text(&input.shadow_registration.registration, "family").as_deref() != Some("rain")
    {
        return Err(Error::Input);
    }
    validate_access(
        input.access,
        input.plan,
        input.confirmation_registration,
        &wire,
    )?;
    let registration_sha256 =
        text(input.confirmation_registration, "registrationSha256").ok_or(Error::Input)?;
    let shadow_registration_sha256 = text(
        &input.shadow_registration.registration,
        "registrationSha256",
    )
    .ok_or(Error::Input)?;
    let parts = expected_rain_parts(
        &text(&input.shadow_registration.registration, "intervalStartAt").ok_or(Error::Input)?,
        &text(&input.shadow_registration.registration, "intervalEndAt").ok_or(Error::Input)?,
        &wire,
    )?;
    let cutoff = text(input.plan, "targetCutoffAt").ok_or(Error::Plan)?;
    let indexes = build_rain_history_indexes(
        input.history,
        &shadow_registration_sha256,
        input.verified_epoch_witness,
        &cutoff,
        &wire,
    )?;
    let mut chunks = Vec::with_capacity(parts.len());
    let mut graph_bindings = Vec::new();
    let mut graph_manifest_sha256s = BTreeSet::new();
    let mut missing_keys = Vec::new();
    let mut rain_gate_input_member_sha256s = BTreeSet::new();
    let mut required_member_identity_sha256s = BTreeSet::new();
    let mut target_member_sha256s = BTreeSet::new();
    // preserve every preregistered physical part regardless of source completeness
    for part in &parts {
        let mut records = Vec::new();
        let mut part_missing_keys = Vec::new();
        // resolve every expected cell against the same authenticated snapshot
        for key in &part.expected_keys {
            let (due_key, row_index, lead_hours) =
                parse_rain_expected_key(key).ok_or(Error::Input)?;
            let resolved = resolve_rain_key(key, &due_key, row_index, lead_hours, &indexes, &wire)?;
            // retain a permanent post-burn gap instead of substituting a later value
            let Some(resolved) = resolved else {
                part_missing_keys.push(key.clone());
                missing_keys.push(key.clone());
                continue;
            };
            graph_bindings.push(resolved.graph_binding);
            graph_manifest_sha256s.extend(resolved.graph_manifest_sha256s);
            rain_gate_input_member_sha256s.insert(resolved.rain_gate_input_member_sha256);
            required_member_identity_sha256s.extend(resolved.required_member_identity_sha256s);
            target_member_sha256s.insert(resolved.target_member_sha256);
            records.push(resolved.record);
        }
        chunks.push(build_chunk(
            part,
            &registration_sha256,
            &shadow_registration_sha256,
            records,
            part_missing_keys,
            &wire,
        )?);
    }
    graph_bindings.sort_by_key(|left| text(left, "key"));
    missing_keys.sort();
    let graph_manifest_sha256s: Vec<_> = graph_manifest_sha256s.into_iter().collect();
    let required_member_identity_sha256s: Vec<_> =
        required_member_identity_sha256s.into_iter().collect();
    let target_member_sha256s: Vec<_> = target_member_sha256s.into_iter().collect();
    let rain_gate_input_member_sha256s: Vec<_> =
        rain_gate_input_member_sha256s.into_iter().collect();
    let derived_targets: Vec<NativeMaintenanceDailyDerivedTarget> = Vec::new();
    // source rain assembly closes any post-burn missing set as unsupported
    if !missing_keys.is_empty() {
        let missing_values = JsonValue::Array(missing_keys.iter().map(|key| string(key)).collect());
        let missing_key_set_sha256 = hash_value(&wire, &missing_values)?;
        let terminal_graph = terminal_graph(
            "adjustment-maintenance-confirmation-unsupported-terminal-graph/v1",
            graph_bindings,
            &target_member_sha256s,
            &graph_manifest_sha256s,
            Some(missing_key_set_sha256.as_str()),
            &wire,
        )?;
        return Ok(NativeMaintenanceDailyTerminalValues::Unsupported {
            chunks,
            contract_version: "adjustment-maintenance-unsupported-confirmation-values/v1",
            derived_targets,
            family: NativeMaintenanceCaptureFamily::Rain,
            graph_manifest_sha256s,
            missing_key_count: missing_keys.len(),
            missing_key_set_sha256,
            registration_sha256,
            rain_gate_input_member_sha256s,
            required_member_identity_sha256s,
            terminal_graph,
        });
    }
    let metadata: Vec<_> = chunks.iter().map(|chunk| chunk.metadata.clone()).collect();
    let full_manifest = assemble_manifest(
        input.access,
        input.confirmation_registration,
        &metadata,
        &wire,
    )?;
    let terminal_graph = terminal_graph(
        "adjustment-maintenance-confirmation-terminal-graph/v1",
        graph_bindings,
        &target_member_sha256s,
        &graph_manifest_sha256s,
        None,
        &wire,
    )?;
    Ok(NativeMaintenanceDailyTerminalValues::Complete {
        chunks,
        contract_version: "adjustment-maintenance-confirmation-values/v1",
        derived_targets,
        family: NativeMaintenanceCaptureFamily::Rain,
        full_manifest,
        graph_manifest_sha256s,
        registration_sha256,
        rain_gate_input_member_sha256s,
        required_member_identity_sha256s,
        terminal_graph,
    })
}

/// Recreate every exact rain physical key part from the frozen provider cadence.
fn expected_rain_parts(
    interval_start_at: &str,
    interval_end_at: &str,
    wire: &NativeWirePrimitives,
) -> Result<Vec<ExpectedPart>, NativeMaintenanceDailyTerminalValuesError> {
    use NativeMaintenanceDailyTerminalValuesError::Input;

    let calendar = NativePredecessorModelCalendar::new().ok_or(Input)?;
    let start = wire
        .canonical_utc_milliseconds(interval_start_at)
        .ok_or(Input)?;
    let end = wire
        .canonical_utc_milliseconds(interval_end_at)
        .ok_or(Input)?;
    let start_local = calendar
        .features(interval_start_at)
        .map_err(|_| Input)?
        .local_date;
    let end_local = calendar
        .features(interval_end_at)
        .map_err(|_| Input)?
        .local_date;
    let start_day = date_day(&start_local, wire).ok_or(Input)?;
    let end_day = date_day(&end_local, wire).ok_or(Input)?;
    // require the reviewed 334-day local-date interval
    if end <= start || end_day - start_day != 334 {
        return Err(Input);
    }
    let first_cycle = start.checked_sub(31 * HOUR).ok_or(Input)?.div_euclid(CYCLE) * CYCLE;
    let mut groups = BTreeMap::<(usize, String), Vec<String>>::new();
    let mut cycle = first_cycle;
    // enumerate fixed source cycles without consulting outcomes
    while cycle < end {
        let due_at = wire
            .format_utc_milliseconds(cycle.checked_add(35 * 60_000).ok_or(Input)?)
            .ok_or(Input)?;
        let due_key = format!("capture/{due_at}");
        let capture_local_date = calendar.features(&due_at).map_err(|_| Input)?.local_date;
        // retain the exact operational rows for model leads nine through thirty-one
        for row_index in 0_i64..23 {
            let model_lead = row_index + 9;
            let valid = cycle.checked_add(model_lead * HOUR).ok_or(Input)?;
            // omit cells outside the half-open registered target interval
            if valid < start || valid >= end {
                continue;
            }
            let valid_at = wire.format_utc_milliseconds(valid).ok_or(Input)?;
            let target_date = calendar.features(&valid_at).map_err(|_| Input)?.local_date;
            let logical_index = usize::try_from(
                (date_day(&target_date, wire).ok_or(Input)? - start_day).div_euclid(14),
            )
            .map_err(|_| Input)?;
            groups
                .entry((logical_index, capture_local_date.clone()))
                .or_default()
                .push(format!("{due_key}/{}/rain", row_index + 1));
        }
        cycle = cycle.checked_add(CYCLE).ok_or(Input)?;
    }
    let mut parts = Vec::new();
    // materialize deterministic target-window and capture-date parts
    for logical_index in 0_usize..24 {
        let from_day = start_day
            .checked_add(i64::try_from(logical_index).map_err(|_| Input)? * 14)
            .ok_or(Input)?;
        let to_day =
            (start_day + i64::try_from(logical_index + 1).map_err(|_| Input)? * 14).min(end_day);
        let from_local_date = format_day(from_day, wire).ok_or(Input)?;
        let to_local_date_exclusive = format_day(to_day, wire).ok_or(Input)?;
        let group: Vec<_> = groups
            .iter()
            .filter(|((index, _), _)| *index == logical_index)
            .collect();
        // require one bounded physical population for every logical window
        if group.is_empty() || group.len() > 24 {
            return Err(Input);
        }
        let part_count = group.len();
        // preserve capture-date order within every logical window
        for (part_index, ((_, capture_local_date), keys)) in group.into_iter().enumerate() {
            let mut expected_keys = keys.clone();
            expected_keys.sort();
            // reject duplicate or oversized fixed-cycle key populations
            if expected_keys.len() > 8_192
                || expected_keys.windows(2).any(|pair| pair[0] == pair[1])
            {
                return Err(Input);
            }
            parts.push(ExpectedPart {
                capture_local_date: capture_local_date.clone(),
                expected_keys,
                from_local_date: from_local_date.clone(),
                logical_chunk_index: logical_index,
                part_count,
                part_index,
                to_local_date_exclusive: to_local_date_exclusive.clone(),
            });
        }
    }
    Ok(parts)
}

/// Build one exact deduplicated rain v3 physical part and its metadata.
fn build_chunk(
    part: &ExpectedPart,
    confirmation_registration_sha256: &str,
    shadow_registration_sha256: &str,
    records: Vec<JsonValue>,
    missing_keys: Vec<String>,
    wire: &NativeWirePrimitives,
) -> Result<NativeMaintenanceDailyTerminalChunk, NativeMaintenanceDailyTerminalValuesError> {
    use NativeMaintenanceDailyTerminalValuesError::Chunk;

    let mut capsules = BTreeMap::<String, JsonValue>::new();
    let mut projected_records = Vec::with_capacity(records.len());
    // deduplicate exact capsule bytes while preserving expected-key order
    for record in records {
        let capsule_base64 = text(&record, "capsuleBase64").ok_or(Chunk)?;
        let capsule_bytes = wire.decode_standard_base64(&capsule_base64).ok_or(Chunk)?;
        let capsule_member_sha256 = format_digest(wire.sha256(&capsule_bytes));
        capsules
            .entry(capsule_member_sha256.clone())
            .or_insert_with(|| {
                object(vec![
                    ("capsuleBase64", string(&capsule_base64)),
                    ("capsuleMemberSha256", string(&capsule_member_sha256)),
                ])
            });
        projected_records.push(object(vec![
            ("capsuleMemberSha256", string(&capsule_member_sha256)),
            (
                "comparison",
                field(&record, "comparison").cloned().ok_or(Chunk)?,
            ),
            ("key", string(&text(&record, "key").ok_or(Chunk)?)),
            ("metric", field(&record, "metric").cloned().ok_or(Chunk)?),
            (
                "rowIndex",
                field(&record, "rowIndex").cloned().ok_or(Chunk)?,
            ),
        ]));
    }
    let record_keys: Vec<_> = projected_records
        .iter()
        .map(|record| text(record, "key").ok_or(Chunk))
        .collect::<Result<_, _>>()?;
    // retain unique source-established key order in every physical part
    if record_keys.windows(2).any(|pair| pair[0] >= pair[1])
        || missing_keys.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return Err(Chunk);
    }
    let mut expected_keys = record_keys.clone();
    expected_keys.extend(missing_keys.iter().cloned());
    expected_keys.sort();
    // prove the eligible and missing sets equal the preregistration
    if expected_keys != part.expected_keys
        || expected_keys.windows(2).any(|pair| pair[0] == pair[1])
    {
        return Err(Chunk);
    }
    let payload = object(vec![
        (
            "capsules",
            JsonValue::Array(capsules.into_values().collect()),
        ),
        ("captureLocalDate", string(&part.capture_local_date)),
        (
            "confirmationRegistrationSha256",
            string(confirmation_registration_sha256),
        ),
        (
            "contractVersion",
            string("adjustment-maintenance-evaluation-chunk/v3"),
        ),
        (
            "expectedKeys",
            JsonValue::Array(expected_keys.iter().map(|key| string(key)).collect()),
        ),
        ("family", string("rain")),
        (
            "logicalChunkIndex",
            JsonValue::Number(part.logical_chunk_index as f64),
        ),
        (
            "missingKeys",
            JsonValue::Array(missing_keys.iter().map(|key| string(key)).collect()),
        ),
        ("partCount", JsonValue::Number(part.part_count as f64)),
        ("partIndex", JsonValue::Number(part.part_index as f64)),
        ("records", JsonValue::Array(projected_records)),
        (
            "shadowRegistrationSha256",
            string(shadow_registration_sha256),
        ),
    ]);
    let payload_bytes = canonical_bytes(wire, &payload)?;
    // preserve the source physical archive member ceiling
    if payload_bytes.len() > MAXIMUM_PART_BYTES {
        return Err(Chunk);
    }
    let metadata = object(vec![
        ("captureLocalDate", string(&part.capture_local_date)),
        (
            "contractVersion",
            string("adjustment-confirmation-chunk-part/v3"),
        ),
        (
            "eligiblePredictionSubsetSha256",
            string(&hash_string_array(wire, &record_keys)?),
        ),
        (
            "expectedKeyCount",
            JsonValue::Number(expected_keys.len() as f64),
        ),
        (
            "expectedKeySubsetSha256",
            string(&hash_string_array(wire, &expected_keys)?),
        ),
        ("fromLocalDate", string(&part.from_local_date)),
        (
            "logicalChunkIndex",
            JsonValue::Number(part.logical_chunk_index as f64),
        ),
        (
            "missingKeyCount",
            JsonValue::Number(missing_keys.len() as f64),
        ),
        (
            "missingKeySubsetSha256",
            string(&hash_string_array(wire, &missing_keys)?),
        ),
        ("partCount", JsonValue::Number(part.part_count as f64)),
        ("partIndex", JsonValue::Number(part.part_index as f64)),
        (
            "partSha256",
            string(&format_digest(wire.sha256(&payload_bytes))),
        ),
        ("recordCount", JsonValue::Number(record_keys.len() as f64)),
        (
            "toLocalDateExclusive",
            string(&part.to_local_date_exclusive),
        ),
    ]);
    Ok(NativeMaintenanceDailyTerminalChunk {
        metadata,
        payload_base64: wire.encode_standard_base64(&payload_bytes).ok_or(Chunk)?,
    })
}

/// Assemble the exact 24-member rain full manifest from physical metadata.
fn assemble_manifest(
    access: &JsonValue,
    registration: &JsonValue,
    parts: &[JsonValue],
    wire: &NativeWirePrimitives,
) -> Result<JsonValue, NativeMaintenanceDailyTerminalValuesError> {
    use NativeMaintenanceDailyTerminalValuesError::Chunk;

    let mut logical_chunks = Vec::with_capacity(24);
    let mut expected_roots = Vec::with_capacity(24);
    // fold consecutive physical parts into each frozen logical window
    for logical_index in 0_usize..24 {
        let group: Vec<_> = parts
            .iter()
            .filter(|part| integer(part, "logicalChunkIndex") == Some(logical_index))
            .collect();
        // require one complete ordered part sequence
        if group.is_empty()
            || group.len() != integer(group[0], "partCount").ok_or(Chunk)?
            || group.iter().enumerate().any(|(index, part)| {
                integer(part, "partIndex") != Some(index)
                    || integer(part, "partCount") != Some(group.len())
            })
        {
            return Err(Chunk);
        }
        let eligible_roots = texts(&group, "eligiblePredictionSubsetSha256")?;
        let part_expected_roots = texts(&group, "expectedKeySubsetSha256")?;
        let missing_roots = texts(&group, "missingKeySubsetSha256")?;
        let part_roots = texts(&group, "partSha256")?;
        let expected_root = hash_string_array(wire, &part_expected_roots)?;
        expected_roots.push(expected_root.clone());
        logical_chunks.push(object(vec![
            (
                "contractVersion",
                string("adjustment-confirmation-logical-chunk/v3"),
            ),
            (
                "eligiblePredictionSubsetSha256",
                string(&hash_string_array(wire, &eligible_roots)?),
            ),
            (
                "expectedKeyCount",
                JsonValue::Number(sum_integer(&group, "expectedKeyCount")? as f64),
            ),
            ("expectedKeySubsetSha256", string(&expected_root)),
            (
                "fromLocalDate",
                string(&text(group[0], "fromLocalDate").ok_or(Chunk)?),
            ),
            ("logicalChunkIndex", JsonValue::Number(logical_index as f64)),
            (
                "missingKeyCount",
                JsonValue::Number(sum_integer(&group, "missingKeyCount")? as f64),
            ),
            (
                "missingKeySubsetSha256",
                string(&hash_string_array(wire, &missing_roots)?),
            ),
            ("partCount", JsonValue::Number(group.len() as f64)),
            (
                "partRootSha256",
                string(&hash_string_array(wire, &part_roots)?),
            ),
            (
                "recordCount",
                JsonValue::Number(sum_integer(&group, "recordCount")? as f64),
            ),
            (
                "toLocalDateExclusive",
                string(&text(group[0], "toLocalDateExclusive").ok_or(Chunk)?),
            ),
        ]));
    }
    let expected_key_set_sha256 = hash_string_array(wire, &expected_roots)?;
    // cross-bind burn authority and preregistration to the reconstructed hierarchy
    if text(access, "expectedKeySetSha256").as_deref() != Some(expected_key_set_sha256.as_str())
        || text(registration, "reservedKeySha256").as_deref()
            != Some(expected_key_set_sha256.as_str())
    {
        return Err(Chunk);
    }
    let eligible_roots: Vec<_> = logical_chunks
        .iter()
        .map(|chunk| text(chunk, "eligiblePredictionSubsetSha256").ok_or(Chunk))
        .collect::<Result<_, _>>()?;
    let missing_roots: Vec<_> = logical_chunks
        .iter()
        .map(|chunk| text(chunk, "missingKeySubsetSha256").ok_or(Chunk))
        .collect::<Result<_, _>>()?;
    let unsigned = object(vec![
        (
            "accessSha256",
            string(&text(access, "accessSha256").ok_or(Chunk)?),
        ),
        ("chunkCount", JsonValue::Number(24.0)),
        ("chunks", JsonValue::Array(logical_chunks)),
        (
            "contractVersion",
            string("adjustment-confirmation-member/v3"),
        ),
        (
            "eligiblePredictionSetSha256",
            string(&hash_string_array(wire, &eligible_roots)?),
        ),
        ("expectedKeySetSha256", string(&expected_key_set_sha256)),
        ("family", string("rain")),
        (
            "intervalEndExclusiveLocalDate",
            string(&text(registration, "intervalEndExclusiveLocalDate").ok_or(Chunk)?),
        ),
        (
            "intervalStartLocalDate",
            string(&text(registration, "intervalStartLocalDate").ok_or(Chunk)?),
        ),
        (
            "missingKeySetSha256",
            string(&hash_string_array(wire, &missing_roots)?),
        ),
        (
            "registrationSha256",
            string(&text(registration, "registrationSha256").ok_or(Chunk)?),
        ),
        (
            "revisionCatalogWatermarkSha256",
            string(&text(access, "revisionCatalogWatermarkSha256").ok_or(Chunk)?),
        ),
        (
            "targetComparatorSnapshotRootSha256",
            string(&text(access, "targetComparatorSnapshotRootSha256").ok_or(Chunk)?),
        ),
        (
            "targetCutoffAt",
            string(&text(access, "targetCutoffAt").ok_or(Chunk)?),
        ),
    ]);
    with_field(
        &unsigned,
        "fullMemberRootSha256",
        string(&hash_value(wire, &unsigned)?),
    )
    .ok_or(Chunk)
}

/// Authenticate and index the exact capsule, control, target, and Best Match population.
fn build_rain_history_indexes(
    history: &NativeMaintenanceHistoricalArchiveIndex,
    registration_sha256: &str,
    witness: &VerifiedNativeMaintenanceCaptureEpochWitness,
    target_cutoff_at: &str,
    wire: &NativeWirePrimitives,
) -> Result<RainHistoryIndexes, NativeMaintenanceDailyTerminalValuesError> {
    use NativeMaintenanceDailyTerminalValuesError as Error;

    let epoch = wire
        .canonical_utc_milliseconds(&witness.epoch_at)
        .ok_or(Error::History)?;
    let cutoff = wire
        .canonical_utc_milliseconds(target_cutoff_at)
        .ok_or(Error::History)?;
    let mut actual_best_matches = HashMap::<String, Vec<RainBestMatch>>::new();
    let mut capsules = HashMap::<String, RainCapsule>::new();
    let mut controls = HashMap::<String, Vec<RainControl>>::new();
    let mut targets = HashMap::<String, Vec<RainTarget>>::new();
    // retain only published contracts used by rain terminal comparison
    for occurrence in &history.occurrences {
        if occurrence.publication_disposition != "published" {
            continue;
        }
        match occurrence.payload_kind.as_str() {
            CAPSULE_VERSION => {
                let capsule = parse_rain_capsule(occurrence, epoch, cutoff, wire)?;
                // ignore capsules from another frozen registration
                if text(&capsule.body, "registrationSha256").as_deref() != Some(registration_sha256)
                {
                    continue;
                }
                let due_key = text(&capsule.body, "dueKey").ok_or(Error::Capsule)?;
                // reject two distinct capsule identities for one due key
                if capsules.get(&due_key).is_some_and(|previous| {
                    previous.payload_identity_sha256 != capsule.payload_identity_sha256
                }) {
                    return Err(Error::Capsule);
                }
                capsules.insert(due_key, capsule);
            }
            CONTROL_VERSION => {
                let control = parse_rain_control(occurrence, epoch, cutoff, wire)?;
                let run_initialized_at = text(
                    field(&control.document, "logicalKey").ok_or(Error::Projection)?,
                    "runInitializedAt",
                )
                .ok_or(Error::Projection)?;
                controls
                    .entry(run_initialized_at)
                    .or_default()
                    .push(control);
            }
            TARGET_VERSION => {
                let target = parse_rain_target(occurrence, epoch, cutoff, wire)?;
                let valid_at = text(&target.document, "validAt").ok_or(Error::Target)?;
                targets.entry(valid_at).or_default().push(target);
            }
            REVISION_V1_VERSION | REVISION_V2_VERSION => {
                // index only canonical actual Best Match rows
                for member in parse_rain_best_match(occurrence, epoch, cutoff, wire)? {
                    let valid_at = text(&member.row, "validAt").ok_or(Error::Projection)?;
                    actual_best_matches
                        .entry(valid_at)
                        .or_default()
                        .push(member);
                }
            }
            _ => {}
        }
    }
    // freeze target order by first complete native archive receipt
    for groups in targets.values_mut() {
        groups.sort_by(|left, right| {
            decimal_cmp(
                &left.maximum_archive_ordinal,
                &right.maximum_archive_ordinal,
            )
            .then(
                left.payload_identity_sha256
                    .cmp(&right.payload_identity_sha256),
            )
        });
        // reject ambiguous distinct target groups at one terminal ordinal
        if groups.len() > 1
            && groups[0].maximum_archive_ordinal == groups[1].maximum_archive_ordinal
            && groups[0].payload_identity_sha256 != groups[1].payload_identity_sha256
        {
            return Err(Error::Target);
        }
    }
    Ok(RainHistoryIndexes {
        actual_best_matches,
        capsules,
        controls,
        targets,
    })
}

/// Parse one archived rain capsule while preserving its database receipt.
fn parse_rain_capsule(
    occurrence: &NativeMaintenanceHistoricalOccurrence,
    epoch: i64,
    cutoff: i64,
    wire: &NativeWirePrimitives,
) -> Result<RainCapsule, NativeMaintenanceDailyTerminalValuesError> {
    use NativeMaintenanceDailyTerminalValuesError as Error;

    validate_occurrence(occurrence, wire)?;
    let codec = NativeShadowProducerCodec::new(NativeModelEvidenceValues::new());
    let capsule =
        PrivateShadowReceiptCodec::decode_capsule_document(&codec, &occurrence.payload_bytes)
            .ok_or(Error::Capsule)?;
    // require the full producer admission before reading any nested value
    if !PrivateShadowArchiveCodec::admit_stage_input(&codec, &capsule.stage_input)
        || occurrence.receipts.len() != 1
        || occurrence.receipts[0] != capsule.revision_receipt
    {
        return Err(Error::Capsule);
    }
    let body = wire
        .decode_json(&capsule.stage_input.body)
        .ok_or(Error::Capsule)?;
    let source = wire
        .decode_json(&capsule.stage_input.sourceProjection)
        .ok_or(Error::Capsule)?;
    let comparator_bytes = match &capsule.stage_input.comparator {
        FieldPresence::Value(bytes) => bytes.clone(),
        FieldPresence::Missing => return Err(Error::Capsule),
    };
    let comparator = wire.decode_json(&comparator_bytes).ok_or(Error::Capsule)?;
    let source_bytes = capsule.stage_input.sourceProjection.clone();
    let prediction_committed_at = match &capsule.prediction_committed_at {
        FieldPresence::Value(value) => value.clone(),
        FieldPresence::Missing => return Err(Error::Capsule),
    };
    // retain only the exact v2 rain source/body/comparator population
    if text(&body, "family").as_deref() != Some("rain")
        || text(&source, "family").as_deref() != Some("rain")
        || text(&body, "dueKey") != Some(capsule.stage_input.metadata.dueKey.clone())
        || text(&body, "registrationSha256")
            != Some(capsule.stage_input.metadata.registrationSha256.clone())
        || format_digest(wire.sha256(&capsule.stage_input.body))
            != capsule.stage_input.metadata.predictionBodySha256
        || format_digest(wire.sha256(&capsule.stage_input.sourceProjection))
            != capsule.stage_input.sourceProjectionSha256
    {
        return Err(Error::Capsule);
    }
    validate_receipt(
        &capsule.revision_receipt,
        "shadow_prediction",
        &capsule.stage_input.metadata.sourceReceiptSha256,
        &capsule.stage_input.metadata.inputSha256,
        epoch,
        cutoff,
        wire,
    )?;
    validate_rain_source_clocks(&source, epoch, cutoff, wire)?;
    // require publication after prediction and inside the frozen snapshot
    if outside_snapshot(&prediction_committed_at, epoch, cutoff, wire)
        || wire
            .canonical_utc_milliseconds(&prediction_committed_at)
            .zip(wire.canonical_utc_milliseconds(&capsule.revision_receipt.archiveCommittedAt))
            .is_none_or(|(prediction, receipt)| prediction > receipt)
    {
        return Err(Error::Capsule);
    }
    Ok(RainCapsule {
        body,
        capsule,
        comparator,
        comparator_bytes,
        graph_manifest_sha256: occurrence.graph_manifest_sha256.clone(),
        payload_bytes: occurrence.payload_bytes.clone(),
        payload_identity_sha256: occurrence.payload_identity_sha256.clone(),
        prediction_committed_at,
        receipt: occurrence.receipts[0].clone(),
        source,
        source_bytes,
    })
}

/// Parse one pre-target rain control projection and its single receipt.
fn parse_rain_control(
    occurrence: &NativeMaintenanceHistoricalOccurrence,
    epoch: i64,
    cutoff: i64,
    wire: &NativeWirePrimitives,
) -> Result<RainControl, NativeMaintenanceDailyTerminalValuesError> {
    use NativeMaintenanceDailyTerminalValuesError as Error;

    validate_occurrence(occurrence, wire)?;
    let decoded =
        decode_native_api_revision_document(&occurrence.payload_bytes).ok_or(Error::Projection)?;
    // require the typed control variant and one database receipt
    if !matches!(
        decoded,
        ApiMaintenanceRevisionTransportDocument::Standard(document)
            if matches!(document.as_ref(), AdjustmentRevisionProjectionDocument::RainControl(_))
    ) || occurrence.receipts.len() != 1
    {
        return Err(Error::Projection);
    }
    let document = wire
        .decode_json(&occurrence.payload_bytes)
        .ok_or(Error::Projection)?;
    let receipt = &occurrence.receipts[0];
    validate_receipt(
        receipt,
        "rain_gate_input",
        &occurrence.payload_identity_sha256,
        &occurrence.payload_identity_sha256,
        epoch,
        cutoff,
        wire,
    )?;
    let logical_key = field(&document, "logicalKey").ok_or(Error::Projection)?;
    let logical_received_at = text(&document, "logicalReceivedAt").ok_or(Error::Projection)?;
    // validate control clocks before indexing their values
    if outside_snapshot(
        &text(logical_key, "runInitializedAt").ok_or(Error::Projection)?,
        epoch,
        cutoff,
        wire,
    ) || outside_snapshot(&logical_received_at, epoch, cutoff, wire)
    {
        return Err(Error::Projection);
    }
    let logical_received = wire
        .canonical_utc_milliseconds(&logical_received_at)
        .ok_or(Error::Projection)?;
    let receipt_at = wire
        .canonical_utc_milliseconds(&receipt.archiveCommittedAt)
        .ok_or(Error::Projection)?;
    let rows = array(&document, "rows").ok_or(Error::Projection)?;
    // require every control to be available strictly before its target
    if logical_received > receipt_at
        || rows.iter().any(|row| {
            wire.canonical_utc_milliseconds(&text(row, "validAt").unwrap_or_default())
                .is_none_or(|valid| logical_received >= valid || receipt_at >= valid)
        })
    {
        return Err(Error::Projection);
    }
    Ok(RainControl {
        document,
        graph_manifest_sha256: occurrence.graph_manifest_sha256.clone(),
        payload_identity_sha256: occurrence.payload_identity_sha256.clone(),
        receipt: receipt.clone(),
    })
}

/// Parse one complete fixed-gauge target and all twelve native receipts.
fn parse_rain_target(
    occurrence: &NativeMaintenanceHistoricalOccurrence,
    epoch: i64,
    cutoff: i64,
    wire: &NativeWirePrimitives,
) -> Result<RainTarget, NativeMaintenanceDailyTerminalValuesError> {
    use NativeMaintenanceDailyTerminalValuesError as Error;

    validate_occurrence(occurrence, wire)?;
    let decoded =
        decode_native_api_revision_document(&occurrence.payload_bytes).ok_or(Error::Target)?;
    let ApiMaintenanceRevisionTransportDocument::FixedGauge(projection) = decoded else {
        return Err(Error::Target);
    };
    // preserve the complete twelve-row receipt population
    if occurrence.receipts.len() != 12 {
        return Err(Error::Target);
    }
    let document = wire
        .decode_json(&occurrence.payload_bytes)
        .ok_or(Error::Target)?;
    // bind target clocks to the future-only snapshot
    if outside_snapshot(&projection.logicalReceivedAt, epoch, cutoff, wire)
        || outside_snapshot(&projection.validAt, epoch, cutoff, wire)
    {
        return Err(Error::Target);
    }
    let logical_received = wire
        .canonical_utc_milliseconds(&projection.logicalReceivedAt)
        .ok_or(Error::Target)?;
    let mut members = Vec::with_capacity(12);
    let mut maximum_archive_ordinal = String::from("0");
    // bind every target row to its exact database receipt
    for (row_index, receipt) in occurrence.receipts.iter().enumerate() {
        validate_receipt(
            receipt,
            "target_revision",
            &occurrence.payload_identity_sha256,
            &occurrence.payload_identity_sha256,
            epoch,
            cutoff,
            wire,
        )?;
        // require the archived target after raw graph availability
        if wire
            .canonical_utc_milliseconds(&receipt.archiveCommittedAt)
            .is_none_or(|receipt_at| receipt_at < logical_received)
        {
            return Err(Error::Target);
        }
        if decimal_cmp(&receipt.archiveCommitOrdinal, &maximum_archive_ordinal) == Ordering::Greater
        {
            maximum_archive_ordinal = receipt.archiveCommitOrdinal.clone();
        }
        members.push(RainTargetMember {
            member_sha256: archive_row_member_sha256(
                receipt,
                &occurrence.payload_identity_sha256,
                "target_revision",
                row_index,
                wire,
            )?,
            receipt: receipt.clone(),
        });
    }
    // require every embedded raw claim to remain inside the snapshot
    for body in &projection.captureBodies {
        for claim in &body.claims {
            for clock in [
                &claim.completedAt,
                &claim.windowStart,
                &claim.windowEndExclusive,
            ] {
                if outside_snapshot(clock, epoch, cutoff, wire) {
                    return Err(Error::Target);
                }
            }
        }
    }
    // require every normalized interval clock inside the snapshot
    for row in &projection.rows {
        for interval in &row.intervals {
            if outside_snapshot(&interval.completedAt, epoch, cutoff, wire)
                || outside_snapshot(&interval.validAt, epoch, cutoff, wire)
            {
                return Err(Error::Target);
            }
        }
    }
    let mut available = projection
        .rows
        .iter()
        .enumerate()
        .filter_map(
            |(index, row)| match &row.normalizedRecord.metrics.precipitationMm {
                Nullable::Null => None,
                Nullable::Value(value) => Some((index, *value)),
            },
        )
        .collect::<Vec<_>>();
    let actual = fixed_gauge_weighted_median(&mut available);
    Ok(RainTarget {
        actual,
        document,
        graph_manifest_sha256: occurrence.graph_manifest_sha256.clone(),
        maximum_archive_ordinal,
        members,
        payload_identity_sha256: occurrence.payload_identity_sha256.clone(),
    })
}

/// Parse canonical serving Best Match rows into their five-key archive members.
fn parse_rain_best_match(
    occurrence: &NativeMaintenanceHistoricalOccurrence,
    epoch: i64,
    cutoff: i64,
    wire: &NativeWirePrimitives,
) -> Result<Vec<RainBestMatch>, NativeMaintenanceDailyTerminalValuesError> {
    use NativeMaintenanceDailyTerminalValuesError as Error;

    validate_occurrence(occurrence, wire)?;
    let decoded =
        decode_native_api_revision_document(&occurrence.payload_bytes).ok_or(Error::Projection)?;
    let ApiMaintenanceRevisionTransportDocument::Standard(document) = decoded else {
        return Ok(Vec::new());
    };
    // fixed-gauge and rain control variants are handled by their own branches
    if !matches!(
        document.as_ref(),
        AdjustmentRevisionProjectionDocument::Single(_)
            | AdjustmentRevisionProjectionDocument::Batch(_)
    ) {
        return Ok(Vec::new());
    }
    let value = wire
        .decode_json(&occurrence.payload_bytes)
        .ok_or(Error::Projection)?;
    // ignore unrelated projections before enforcing Best Match lineage
    if text(&value, "projectionKind").as_deref() != Some("actual_best_match") {
        return Ok(Vec::new());
    }
    let version = text(&value, "contractVersion").ok_or(Error::Projection)?;
    let correct_family = (version == REVISION_V2_VERSION
        && text(&value, "family").as_deref() == Some("wind"))
        || (version == REVISION_V1_VERSION && text(&value, "family").as_deref() == Some("shared"));
    let rows = array(&value, "rows").ok_or(Error::Projection)?;
    let expected_receipts = if version == REVISION_V2_VERSION {
        rows.len()
    } else {
        1
    };
    // close the original single or grouped archive envelope
    if !correct_family || occurrence.receipts.len() != expected_receipts {
        return Err(Error::Projection);
    }
    let source = field(&value, "source").ok_or(Error::Projection)?;
    let logical_key = field(&value, "logicalKey").ok_or(Error::Projection)?;
    // skip a valid noncanonical forecast source without treating it as corruption
    if !rain_best_match_source_is_canonical(source, logical_key) {
        return Ok(Vec::new());
    }
    let product_run_at = text(logical_key, "productRunAt").ok_or(Error::Projection)?;
    let logical_received_at = text(&value, "logicalReceivedAt").ok_or(Error::Projection)?;
    // bind source and receipt clocks to the frozen snapshot
    if outside_snapshot(&product_run_at, epoch, cutoff, wire)
        || outside_snapshot(&logical_received_at, epoch, cutoff, wire)
        || wire
            .canonical_utc_milliseconds(&product_run_at)
            .zip(wire.canonical_utc_milliseconds(&logical_received_at))
            .is_none_or(|(run, received)| run > received)
    {
        return Err(Error::Projection);
    }
    let logical_received = wire
        .canonical_utc_milliseconds(&logical_received_at)
        .ok_or(Error::Projection)?;
    let mut members = Vec::with_capacity(rows.len());
    // retain every canonical source row with its exact receipt
    for (row_index, row) in rows.iter().enumerate() {
        let receipt = occurrence
            .receipts
            .get(if occurrence.receipts.len() == 1 {
                0
            } else {
                row_index
            })
            .ok_or(Error::Projection)?;
        validate_receipt(
            receipt,
            "actual_best_match",
            &occurrence.payload_identity_sha256,
            &occurrence.payload_identity_sha256,
            epoch,
            cutoff,
            wire,
        )?;
        let valid_at = text(row, "validAt").ok_or(Error::Projection)?;
        // require normalized row availability before archive commitment
        if outside_snapshot(&valid_at, epoch, cutoff, wire)
            || wire
                .canonical_utc_milliseconds(&receipt.archiveCommittedAt)
                .is_none_or(|receipt_at| receipt_at < logical_received)
        {
            return Err(Error::Projection);
        }
        members.push(RainBestMatch {
            document: value.clone(),
            graph_manifest_sha256: occurrence.graph_manifest_sha256.clone(),
            member_sha256: archive_row_member_sha256(
                receipt,
                &occurrence.payload_identity_sha256,
                "actual_best_match",
                row_index,
                wire,
            )?,
            payload_identity_sha256: occurrence.payload_identity_sha256.clone(),
            receipt: receipt.clone(),
            row: row.clone(),
        });
    }
    Ok(members)
}

/// Accept only the production v4 Best Match source lineage.
fn rain_best_match_source_is_canonical(source: &JsonValue, logical_key: &JsonValue) -> bool {
    text(source, "adapterVersion").as_deref() == Some("open-meteo-forecast-daily/v4")
        && text(source, "contractEpoch").as_deref()
            == Some("legacy-v4/9d26d9c46dcaacc422c28e854327b11cd710625e092110786010f0687a100d83")
        && text(source, "dataset").as_deref() == Some("best_match")
        && text(source, "providerKey").as_deref() == Some("open-meteo")
        && text(source, "sourceConfigFingerprint").as_deref()
            == Some("ceb83ac4ba3ddc421a31043794ad450a859ecc31643506f93f64a28feb15e5b4")
        && text(source, "sourceId") == text(logical_key, "sourceId")
        && text(source, "sourceKey").as_deref() == Some("open-meteo-forecast-v4")
        && text(source, "sourceKind").as_deref() == Some("forecast")
        && text(source, "upstreamModel").as_deref() == Some("best_match")
}

/// Resolve one preregistered rain key through its exact capsule and related projections.
fn resolve_rain_key(
    key: &str,
    due_key: &str,
    row_index: usize,
    lead_hours: usize,
    indexes: &RainHistoryIndexes,
    wire: &NativeWirePrimitives,
) -> Result<Option<RainResolvedRecord>, NativeMaintenanceDailyTerminalValuesError> {
    use NativeMaintenanceDailyTerminalValuesError::Projection;

    let Some(capsule) = indexes.capsules.get(due_key) else {
        return Ok(None);
    };
    let source_row = array_item(&capsule.source, "rows", row_index).ok_or(Projection)?;
    let reference_at = text(source_row, "referenceAt").ok_or(Projection)?;
    let valid_at = text(source_row, "validAt").ok_or(Projection)?;
    let Some(control) = select_rain_control(
        capsule,
        indexes
            .controls
            .get(&reference_at)
            .map(Vec::as_slice)
            .unwrap_or_default(),
        wire,
    )?
    else {
        return Ok(None);
    };
    resolve_rain_record(
        key,
        row_index,
        lead_hours,
        capsule,
        control,
        indexes
            .targets
            .get(&valid_at)
            .map(Vec::as_slice)
            .unwrap_or_default(),
        indexes
            .actual_best_matches
            .get(&valid_at)
            .map(Vec::as_slice)
            .unwrap_or_default(),
        wire,
    )
}

/// Select the one control whose complete source and incumbent tuples match.
fn select_rain_control<'a>(
    capsule: &RainCapsule,
    controls: &'a [RainControl],
    wire: &NativeWirePrimitives,
) -> Result<Option<&'a RainControl>, NativeMaintenanceDailyTerminalValuesError> {
    let matches = controls
        .iter()
        .filter(|control| rain_control_matches_capsule(capsule, control, wire))
        .collect::<Vec<_>>();
    // reject an ambiguous control instead of choosing by outcome value
    if matches.len() > 1 {
        return Err(NativeMaintenanceDailyTerminalValuesError::Projection);
    }
    Ok(matches.into_iter().next())
}

/// Cross-bind one archived control to the nested source and comparator population.
fn rain_control_matches_capsule(
    capsule: &RainCapsule,
    control: &RainControl,
    wire: &NativeWirePrimitives,
) -> bool {
    let Some(causal) = field(&capsule.source, "causalInputs") else {
        return false;
    };
    let Some(current_run) = field(causal, "currentRun") else {
        return false;
    };
    let Some(current_initialized_at) = text(current_run, "runInitializedAt") else {
        return false;
    };
    let Some(current_content_sha256) = text(current_run, "contentSha256") else {
        return false;
    };
    let Some(captures) = array(causal, "captureSet") else {
        return false;
    };
    let forecast_capture = captures.iter().find(|capture| {
        text(capture, "kind").as_deref() == Some("forecast")
            && text(capture, "runInitializedAt").as_deref() == Some(current_initialized_at.as_str())
            && text(capture, "bodySha256").as_deref() == Some(current_content_sha256.as_str())
    });
    let Some(forecast_capture) = forecast_capture else {
        return false;
    };
    let Some(control_key) = field(&control.document, "logicalKey") else {
        return false;
    };
    let Some(control_source) = field(&control.document, "source") else {
        return false;
    };
    let Some(source_rows) = array(&capsule.source, "rows") else {
        return false;
    };
    let Some(comparator_rows) = array(&capsule.comparator, "rows") else {
        return false;
    };
    let Some(control_rows) = array(&control.document, "rows") else {
        return false;
    };
    let Some(source_first) = source_rows.first() else {
        return false;
    };
    let Some(control_first) = control_rows.first() else {
        return false;
    };
    let Some(authority) = field(&capsule.comparator, "servingAuthority") else {
        return false;
    };
    let source_issued_at = text(&capsule.source, "issuedAt");
    let control_received_at = text(&control.document, "logicalReceivedAt");
    let clocks_match = source_issued_at
        .as_deref()
        .zip(control_received_at.as_deref())
        .zip(Some(capsule.prediction_committed_at.as_str()))
        .is_some_and(|((issued, received), predicted)| {
            wire.canonical_utc_milliseconds(issued)
                .zip(wire.canonical_utc_milliseconds(received))
                .zip(wire.canonical_utc_milliseconds(predicted))
                .zip(wire.canonical_utc_milliseconds(&control.receipt.archiveCommittedAt))
                .is_some_and(|(((issued, received), predicted), receipt)| {
                    received >= issued
                        && received <= predicted
                        && receipt >= received
                        && receipt <= predicted
                })
        });
    // bind run, source lineage, authority, and availability before row comparison
    if !clocks_match
        || text(control_key, "runInitializedAt").as_deref() != Some(current_initialized_at.as_str())
        || text(control_source, "sourceId") != text(forecast_capture, "claimId")
        || !same_text(
            control_source,
            "contractEpoch",
            source_first,
            "contractEpoch",
        )
        || !same_text(control_source, "dataset", source_first, "dataset")
        || !same_text(control_source, "providerKey", source_first, "providerKey")
        || !same_text(
            control_source,
            "sourceConfigFingerprint",
            source_first,
            "sourceConfigFingerprint",
        )
        || !same_text(control_source, "sourceKey", source_first, "sourceKey")
        || !same_text(
            control_source,
            "upstreamModel",
            source_first,
            "upstreamModel",
        )
        || field(authority, "artifactIdentitySha256")
            != field(control_first, "incumbentArtifactIdentitySha256")
        || field(authority, "receiptMemberSha256")
            != field(control_first, "incumbentReceiptMemberSha256")
        || source_rows.len() != comparator_rows.len()
        || source_rows.len() != control_rows.len()
    {
        return false;
    }
    // compare all twenty-three exact source and incumbent tuples
    for ((source_row, comparator_row), control_row) in
        source_rows.iter().zip(comparator_rows).zip(control_rows)
    {
        if text(source_row, "referenceAt").as_deref() != Some(current_initialized_at.as_str())
            || field(source_row, "modelLeadHours") != field(control_row, "modelLeadHours")
            || field(source_row, "validAt") != field(control_row, "validAt")
            || field(source_row, "precipitationMm64") != field(control_row, "rawPrecipitationMm64")
            || field(comparator_row, "incumbentPrecipitationMm64")
                != field(control_row, "incumbentPrediction64")
            || field(comparator_row, "occurrenceProbability64")
                != nested_field(control_row, "incumbentProbability", "atLeast0_1")
            || field(comparator_row, "atLeast1_0Probability64")
                != nested_field(control_row, "incumbentProbability", "atLeast1_0")
            || field(comparator_row, "atLeast2_5Probability64")
                != nested_field(control_row, "incumbentProbability", "atLeast2_5")
        {
            return false;
        }
    }
    true
}

/// Join one rain candidate row to its control, source, Best Match, and target.
#[allow(clippy::too_many_arguments)]
fn resolve_rain_record(
    key: &str,
    row_index: usize,
    lead_hours: usize,
    capsule: &RainCapsule,
    control: &RainControl,
    targets: &[RainTarget],
    actual_best_matches: &[RainBestMatch],
    wire: &NativeWirePrimitives,
) -> Result<Option<RainResolvedRecord>, NativeMaintenanceDailyTerminalValuesError> {
    use NativeMaintenanceDailyTerminalValuesError::Projection;

    // preserve the full control/source/comparator match before selecting one row
    if !rain_control_matches_capsule(capsule, control, wire) {
        return Ok(None);
    }
    let Some(body_row) = array_item(&capsule.body, "rows", row_index) else {
        return Ok(None);
    };
    let Some(source_row) = array_item(&capsule.source, "rows", row_index) else {
        return Ok(None);
    };
    let Some(comparator_row) = array_item(&capsule.comparator, "rows", row_index) else {
        return Ok(None);
    };
    let Some(control_row) = array_item(&control.document, "rows", row_index) else {
        return Ok(None);
    };
    let Some(valid_at) = text(body_row, "validAt") else {
        return Ok(None);
    };
    // bind the operational lead and common target clock across all rows
    if text(&capsule.body, "dueKey").is_none()
        || integer_value(field(body_row, "leadHours")) != Some(lead_hours)
        || integer_value(field(source_row, "leadHours")) != Some(lead_hours)
        || integer_value(field(comparator_row, "leadHours")) != Some(lead_hours)
        || integer_value(field(source_row, "modelLeadHours")) != Some(lead_hours + 8)
        || field(control_row, "modelLeadHours") != field(source_row, "modelLeadHours")
        || text(source_row, "validAt").as_deref() != Some(valid_at.as_str())
        || field(comparator_row, "validAt") != field(body_row, "validAt")
        || field(control_row, "validAt") != field(body_row, "validAt")
    {
        return Ok(None);
    }
    let prediction_committed = wire
        .canonical_utc_milliseconds(&capsule.prediction_committed_at)
        .ok_or(Projection)?;
    let valid = wire
        .canonical_utc_milliseconds(&valid_at)
        .ok_or(Projection)?;
    // require prediction before its target clock
    if prediction_committed >= valid {
        return Ok(None);
    }
    let run_initialized_at = text(source_row, "referenceAt").ok_or(Projection)?;
    let run_initialized = wire
        .canonical_utc_milliseconds(&run_initialized_at)
        .ok_or(Projection)?;
    let minimum_issued = run_initialized.checked_add(8 * HOUR).ok_or(Projection)?;
    let expected_due_at = wire
        .format_utc_milliseconds(run_initialized.checked_add(35 * 60_000).ok_or(Projection)?)
        .ok_or(Projection)?;
    let expected_due_key = format!("capture/{expected_due_at}");
    let source_issued_at = text(&capsule.source, "issuedAt").ok_or(Projection)?;
    let source_issued = wire
        .canonical_utc_milliseconds(&source_issued_at)
        .ok_or(Projection)?;
    let causal = field(&capsule.source, "causalInputs").ok_or(Projection)?;
    let current_run = field(causal, "currentRun").ok_or(Projection)?;
    let captures = array(causal, "captureSet").ok_or(Projection)?;
    let source_content_sha256 = text(source_row, "contentSha256").ok_or(Projection)?;
    let forecast_capture = captures.iter().find(|capture| {
        text(capture, "kind").as_deref() == Some("forecast")
            && text(capture, "runInitializedAt").as_deref() == Some(run_initialized_at.as_str())
            && text(capture, "bodySha256").as_deref() == Some(source_content_sha256.as_str())
    });
    let Some(forecast_capture) = forecast_capture else {
        return Ok(None);
    };
    let control_key = field(&control.document, "logicalKey").ok_or(Projection)?;
    let control_source = field(&control.document, "source").ok_or(Projection)?;
    let control_received_at = text(&control.document, "logicalReceivedAt").ok_or(Projection)?;
    let control_received = wire
        .canonical_utc_milliseconds(&control_received_at)
        .ok_or(Projection)?;
    let control_receipt_at = wire
        .canonical_utc_milliseconds(&control.receipt.archiveCommittedAt)
        .ok_or(Projection)?;
    let capsule_receipt_at = wire
        .canonical_utc_milliseconds(&capsule.receipt.archiveCommittedAt)
        .ok_or(Projection)?;
    // bind causal source, control lineage, and all pre-target availability clocks
    if source_issued < minimum_issued
        || source_issued >= valid
        || text(&capsule.body, "dueKey").as_deref() != Some(expected_due_key.as_str())
        || valid != run_initialized + i64::try_from(lead_hours + 8).map_err(|_| Projection)? * HOUR
        || text(current_run, "runInitializedAt").as_deref() != Some(run_initialized_at.as_str())
        || text(current_run, "contentSha256").as_deref() != Some(source_content_sha256.as_str())
        || text(current_run, "completedAt") != text(forecast_capture, "completedAt")
        || text(control_key, "runInitializedAt").as_deref() != Some(run_initialized_at.as_str())
        || !same_text(control_source, "contractEpoch", source_row, "contractEpoch")
        || !same_text(control_source, "dataset", source_row, "dataset")
        || !same_text(control_source, "providerKey", source_row, "providerKey")
        || !same_text(
            control_source,
            "sourceConfigFingerprint",
            source_row,
            "sourceConfigFingerprint",
        )
        || !same_text(control_source, "sourceKey", source_row, "sourceKey")
        || !same_text(control_source, "upstreamModel", source_row, "upstreamModel")
        || field(control_row, "rawPrecipitationMm64") != field(source_row, "precipitationMm64")
        || control_received < source_issued
        || control_received > prediction_committed
        || control_receipt_at < control_received
        || control_receipt_at > prediction_committed
        || capsule_receipt_at >= valid
    {
        return Ok(None);
    }
    let authority = field(&capsule.comparator, "servingAuthority").ok_or(Projection)?;
    // bind the incumbent comparator tuple to the selected control row
    if field(comparator_row, "incumbentPrecipitationMm64")
        != field(control_row, "incumbentPrediction64")
        || field(comparator_row, "occurrenceProbability64")
            != nested_field(control_row, "incumbentProbability", "atLeast0_1")
        || field(comparator_row, "atLeast1_0Probability64")
            != nested_field(control_row, "incumbentProbability", "atLeast1_0")
        || field(comparator_row, "atLeast2_5Probability64")
            != nested_field(control_row, "incumbentProbability", "atLeast2_5")
        || field(authority, "artifactIdentitySha256")
            != field(control_row, "incumbentArtifactIdentitySha256")
        || field(authority, "receiptMemberSha256")
            != field(control_row, "incumbentReceiptMemberSha256")
    {
        return Ok(None);
    }
    let Some(target) = targets.first() else {
        return Ok(None);
    };
    let Some(target_value) = target.actual else {
        return Ok(None);
    };
    // bind the first complete target group to this exact row
    if text(&target.document, "validAt").as_deref() != Some(valid_at.as_str()) {
        return Ok(None);
    }
    let Some(actual_best_match) =
        select_rain_actual_best_match(actual_best_matches, capsule, target, wire)?
    else {
        return Ok(None);
    };
    let Some(actual_best_match_prediction) =
        optional_binary64(&actual_best_match.row, "precipitationMm64")?
    else {
        return Ok(None);
    };
    let control_member_sha256 = archive_row_member_sha256(
        &control.receipt,
        &control.payload_identity_sha256,
        "rain_gate_input",
        row_index,
        wire,
    )?;
    let native_source_member_sha256 = text(body_row, "sourceRowSha256").ok_or(Projection)?;
    let comparator_member_sha256 = format_digest(wire.sha256(&capsule.comparator_bytes));
    let source_projection_sha256 = format_digest(wire.sha256(&capsule.source_bytes));
    let mut edge_clocks = vec![
        actual_best_match.receipt.archiveCommittedAt.as_str(),
        capsule.prediction_committed_at.as_str(),
        capsule.receipt.archiveCommittedAt.as_str(),
        control.receipt.archiveCommittedAt.as_str(),
    ];
    edge_clocks.extend(
        target
            .members
            .iter()
            .map(|member| member.receipt.archiveCommittedAt.as_str()),
    );
    let first_edge_committed_at = edge_clocks.into_iter().max().ok_or(Projection)?;
    let comparison = object(vec![
        (
            "actualBestMatchPrediction",
            JsonValue::Number(actual_best_match_prediction),
        ),
        ("farmTarget", JsonValue::Null),
        ("firstEdgeCommittedAt", string(first_edge_committed_at)),
        (
            "incumbentPrediction",
            JsonValue::Number(required_binary64(control_row, "incumbentPrediction64")?),
        ),
        (
            "incumbentProbability",
            decode_probability(field(control_row, "incumbentProbability").ok_or(Projection)?)?,
        ),
        (
            "membership",
            object(vec![
                (
                    "actualBestMatchProjectionSha256",
                    string(&actual_best_match.payload_identity_sha256),
                ),
                (
                    "actualBestMatchReceiptSha256",
                    string(&actual_best_match.receipt.receiptSha256),
                ),
                ("incumbentMemberSha256", string(&comparator_member_sha256)),
                (
                    "nativeSourceProjectionSha256",
                    string(&source_projection_sha256),
                ),
                (
                    "nativeSourceReceiptSha256",
                    string(&capsule.receipt.receiptSha256),
                ),
                (
                    "rainGateProjectionSha256",
                    string(&control.payload_identity_sha256),
                ),
                (
                    "rainGateReceiptSha256",
                    string(&control.receipt.receiptSha256),
                ),
                (
                    "targetMemberSha256",
                    string(&target.payload_identity_sha256),
                ),
            ]),
        ),
        (
            "nativeSourceProbability",
            decode_probability(field(control_row, "nativeSourceProbability").ok_or(Projection)?)?,
        ),
        ("nearestThree", JsonValue::Null),
        (
            "persistencePrediction",
            JsonValue::Number(required_binary64(control_row, "persistencePrediction64")?),
        ),
        ("providerFamily", JsonValue::Null),
        (
            "rawTargetHourTemperatureC",
            JsonValue::Number(required_binary64(
                control_row,
                "rawTargetHourTemperatureC64",
            )?),
        ),
        (
            "recentVolumeScalePrediction",
            JsonValue::Number(required_binary64(
                control_row,
                "recentVolumeScalePrediction64",
            )?),
        ),
        (
            "runKey",
            string(&format!("{run_initialized_at}/{valid_at}")),
        ),
        (
            "sameWindowVolumeScalePrediction",
            JsonValue::Number(required_binary64(
                control_row,
                "sameWindowVolumeScalePrediction64",
            )?),
        ),
        ("stationKey", JsonValue::Null),
        ("target", JsonValue::Number(target_value)),
        (
            "unchangedOrdinalPrediction",
            JsonValue::Number(required_binary64(
                control_row,
                "unchangedOrdinalPrediction64",
            )?),
        ),
        (
            "volumeScalePrediction",
            JsonValue::Number(required_binary64(control_row, "volumeScalePrediction64")?),
        ),
    ]);
    let mut required = BTreeSet::from([
        actual_best_match.member_sha256.clone(),
        actual_best_match.payload_identity_sha256.clone(),
        actual_best_match.receipt.receiptSha256.clone(),
        capsule.payload_identity_sha256.clone(),
        capsule.receipt.receiptSha256.clone(),
        format_digest(wire.sha256(&capsule.capsule.stage_input.body)),
        source_projection_sha256.clone(),
        comparator_member_sha256.clone(),
        text(authority, "receiptMemberSha256").ok_or(Projection)?,
        control_member_sha256.clone(),
        control.payload_identity_sha256.clone(),
        control.receipt.receiptSha256.clone(),
        native_source_member_sha256.clone(),
        target.payload_identity_sha256.clone(),
    ]);
    // retain every fixed-gauge row and receipt identity
    for member in &target.members {
        required.insert(member.member_sha256.clone());
        required.insert(member.receipt.receiptSha256.clone());
    }
    // retain the incumbent artifact only when the authority owns one
    if let Some(artifact) = optional_text(authority, "artifactIdentitySha256")? {
        required.insert(artifact);
    }
    let source_members = target
        .members
        .iter()
        .map(|member| string(&member.member_sha256))
        .collect();
    Ok(Some(RainResolvedRecord {
        graph_binding: object(vec![
            (
                "actualBestMatch",
                object(vec![
                    ("memberSha256", string(&actual_best_match.member_sha256)),
                    (
                        "payloadIdentitySha256",
                        string(&actual_best_match.payload_identity_sha256),
                    ),
                    (
                        "receiptSha256",
                        string(&actual_best_match.receipt.receiptSha256),
                    ),
                ]),
            ),
            (
                "capsule",
                object(vec![
                    ("comparatorMemberSha256", string(&comparator_member_sha256)),
                    (
                        "payloadIdentitySha256",
                        string(&capsule.payload_identity_sha256),
                    ),
                    (
                        "predictionBodySha256",
                        string(&format_digest(
                            wire.sha256(&capsule.capsule.stage_input.body),
                        )),
                    ),
                    ("receiptSha256", string(&capsule.receipt.receiptSha256)),
                    ("sourceProjectionSha256", string(&source_projection_sha256)),
                ]),
            ),
            ("key", string(key)),
            (
                "nativeSource",
                object(vec![
                    ("memberSha256", string(&native_source_member_sha256)),
                    ("payloadIdentitySha256", string(&source_projection_sha256)),
                    ("receiptSha256", string(&capsule.receipt.receiptSha256)),
                ]),
            ),
            (
                "target",
                object(vec![
                    ("sourceMemberSha256s", JsonValue::Array(source_members)),
                    (
                        "targetMemberSha256",
                        string(&target.payload_identity_sha256),
                    ),
                ]),
            ),
        ]),
        graph_manifest_sha256s: vec![
            actual_best_match.graph_manifest_sha256.clone(),
            capsule.graph_manifest_sha256.clone(),
            control.graph_manifest_sha256.clone(),
            target.graph_manifest_sha256.clone(),
        ],
        rain_gate_input_member_sha256: control_member_sha256,
        record: object(vec![
            (
                "capsuleBase64",
                string(
                    &wire
                        .encode_standard_base64(&capsule.payload_bytes)
                        .ok_or(Projection)?,
                ),
            ),
            ("comparison", comparison),
            ("key", string(key)),
            ("metric", JsonValue::Null),
            ("rowIndex", JsonValue::Number(row_index as f64)),
        ]),
        required_member_identity_sha256s: required.into_iter().collect(),
        target_member_sha256: target.payload_identity_sha256.clone(),
    }))
}

/// Choose the first canonical Best Match revision available before prediction and target.
fn select_rain_actual_best_match<'a>(
    members: &'a [RainBestMatch],
    capsule: &RainCapsule,
    target: &RainTarget,
    wire: &NativeWirePrimitives,
) -> Result<Option<&'a RainBestMatch>, NativeMaintenanceDailyTerminalValuesError> {
    use NativeMaintenanceDailyTerminalValuesError::Projection;

    let prediction = wire
        .canonical_utc_milliseconds(&capsule.prediction_committed_at)
        .ok_or(Projection)?;
    let target_received = wire
        .canonical_utc_milliseconds(&text(&target.document, "logicalReceivedAt").ok_or(Projection)?)
        .ok_or(Projection)?;
    let mut matches = members
        .iter()
        .filter(|member| {
            let logical_received = text(&member.document, "logicalReceivedAt")
                .and_then(|value| wire.canonical_utc_milliseconds(&value));
            let receipt = wire.canonical_utc_milliseconds(&member.receipt.archiveCommittedAt);
            logical_received
                .zip(receipt)
                .is_some_and(|(logical, receipt)| {
                    logical <= prediction
                        && receipt <= prediction
                        && logical <= target_received
                        && receipt <= target_received
                })
        })
        .collect::<Vec<_>>();
    matches.sort_by(|left, right| {
        decimal_cmp(
            &left.receipt.archiveCommitOrdinal,
            &right.receipt.archiveCommitOrdinal,
        )
        .then(left.member_sha256.cmp(&right.member_sha256))
    });
    // reject two different rows sharing the first archive ordinal
    if matches.len() > 1
        && matches[0].receipt.archiveCommitOrdinal == matches[1].receipt.archiveCommitOrdinal
        && matches[0].member_sha256 != matches[1].member_sha256
    {
        return Err(Projection);
    }
    Ok(matches.into_iter().next())
}

/// Bind the owner burn to the exact blinded plan before value parsing.
fn validate_access(
    access: &JsonValue,
    plan: &JsonValue,
    registration: &JsonValue,
    wire: &NativeWirePrimitives,
) -> Result<(), NativeMaintenanceDailyTerminalValuesError> {
    use NativeMaintenanceDailyTerminalValuesError::Access;

    for name in [
        "accessSha256",
        "expectedKeySetSha256",
        "revisionCatalogWatermarkSha256",
        "targetComparatorSnapshotRootSha256",
    ] {
        if text(access, name).is_none_or(|value| !valid_hash(&value)) {
            return Err(Access);
        }
    }
    let accessed_at = text(access, "accessedAt").ok_or(Access)?;
    let terminal_at = text(registration, "terminalAccessAt").ok_or(Access)?;
    let unsigned = without_field(access, "accessSha256").ok_or(Access)?;
    let expected_access_sha256 = hash_value(wire, &unsigned)?;
    // cross-bind access state, family, plan roots, cutoff, and terminal clock
    if text(access, "accessState").as_deref() != Some("burned")
        || text(access, "family").as_deref() != Some("rain")
        || text(registration, "family").as_deref() != Some("rain")
        || text(access, "registrationSha256") != text(registration, "registrationSha256")
        || text(access, "accessSha256").as_deref() != Some(expected_access_sha256.as_str())
        || text(access, "expectedKeySetSha256") != text(plan, "expectedKeySetSha256")
        || text(access, "revisionCatalogWatermarkSha256")
            != text(plan, "revisionCatalogWatermarkSha256")
        || text(access, "targetComparatorSnapshotRootSha256") != text(plan, "snapshotRootSha256")
        || text(access, "targetCutoffAt") != text(plan, "targetCutoffAt")
        || wire
            .canonical_utc_milliseconds(&accessed_at)
            .zip(wire.canonical_utc_milliseconds(&terminal_at))
            .is_none_or(|(accessed, terminal)| accessed < terminal)
    {
        return Err(Access);
    }
    Ok(())
}

/// Build the exact complete or unsupported rain terminal graph identity.
fn terminal_graph(
    contract_version: &str,
    bindings: Vec<JsonValue>,
    target_member_sha256s: &[String],
    graph_manifest_sha256s: &[String],
    missing_key_set_sha256: Option<&str>,
    wire: &NativeWirePrimitives,
) -> Result<JsonValue, NativeMaintenanceDailyTerminalValuesError> {
    let mut fields = vec![
        ("bindings", JsonValue::Array(bindings)),
        ("contractVersion", string(contract_version)),
        (
            "derivedTargetMemberSha256s",
            JsonValue::Array(
                target_member_sha256s
                    .iter()
                    .map(|identity| string(identity))
                    .collect(),
            ),
        ),
        (
            "graphManifestSha256s",
            JsonValue::Array(
                graph_manifest_sha256s
                    .iter()
                    .map(|identity| string(identity))
                    .collect(),
            ),
        ),
    ];
    // only unsupported graphs bind an explicit complete missing-key root
    if let Some(missing) = missing_key_set_sha256 {
        fields.push(("missingKeySetSha256", string(missing)));
    }
    let unsigned = object(fields);
    with_field(
        &unsigned,
        "terminalGraphSha256",
        string(&hash_value(wire, &unsigned)?),
    )
    .ok_or(NativeMaintenanceDailyTerminalValuesError::Codec)
}

/// Validate one published occurrence envelope before semantic decoding.
fn validate_occurrence(
    occurrence: &NativeMaintenanceHistoricalOccurrence,
    wire: &NativeWirePrimitives,
) -> Result<(), NativeMaintenanceDailyTerminalValuesError> {
    use NativeMaintenanceDailyTerminalValuesError::History;

    // bind every published payload to its immutable graph envelope
    if !valid_hash(&occurrence.graph_manifest_sha256)
        || !valid_hash(&occurrence.page_sha256)
        || !valid_hash(&occurrence.payload_identity_sha256)
        || occurrence.publication_disposition != "published"
        || format_digest(wire.sha256(&occurrence.payload_bytes))
            != occurrence.payload_identity_sha256
    {
        return Err(History);
    }
    Ok(())
}

/// Bind one database receipt to its projection identity and snapshot clocks.
fn validate_receipt(
    receipt: &AdjustmentRevisionCommitReceipt,
    expected_kind: &str,
    identity: &str,
    projection_sha256: &str,
    epoch: i64,
    cutoff: i64,
    wire: &NativeWirePrimitives,
) -> Result<(), NativeMaintenanceDailyTerminalValuesError> {
    use NativeMaintenanceDailyTerminalValuesError::History;

    // reject noncanonical ordinals or detached projection identities
    if !valid_hash(&receipt.receiptSha256)
        || !valid_decimal(&receipt.archiveCommitOrdinal)
        || receipt_kind(receipt.projectionKind) != expected_kind
        || receipt.projectionIdentitySha256 != identity
        || receipt.projectionSha256 != projection_sha256
        || outside_snapshot(&receipt.archiveCommittedAt, epoch, cutoff, wire)
    {
        return Err(History);
    }
    Ok(())
}

/// Validate every nested rain causal clock and source availability relation.
fn validate_rain_source_clocks(
    source: &JsonValue,
    epoch: i64,
    cutoff: i64,
    wire: &NativeWirePrimitives,
) -> Result<(), NativeMaintenanceDailyTerminalValuesError> {
    use NativeMaintenanceDailyTerminalValuesError::Capsule;

    let issued_at = text(source, "issuedAt").ok_or(Capsule)?;
    let issued = wire.canonical_utc_milliseconds(&issued_at).ok_or(Capsule)?;
    // require issuance inside the future-only snapshot
    if outside_snapshot(&issued_at, epoch, cutoff, wire) {
        return Err(Capsule);
    }
    let causal = field(source, "causalInputs").ok_or(Capsule)?;
    let current = field(causal, "currentRun").ok_or(Capsule)?;
    for name in ["runInitializedAt", "completedAt"] {
        validate_clock_field(current, name, epoch, cutoff, wire)?;
    }
    // validate every prior provider run before source issuance
    for run in array(causal, "priorRuns").ok_or(Capsule)? {
        validate_clock_field(run, "runInitializedAt", epoch, cutoff, wire)?;
        validate_clock_field(run, "completedAt", epoch, cutoff, wire)?;
    }
    // validate every retained raw capture clock and optional interval
    for capture in array(causal, "captureSet").ok_or(Capsule)? {
        let completed = validate_clock_field(capture, "completedAt", epoch, cutoff, wire)?;
        if completed > issued {
            return Err(Capsule);
        }
        for name in ["runInitializedAt", "windowStart", "windowEndExclusive"] {
            if let Some(clock) = nullable_text(capture, name)?
                && outside_snapshot(&clock, epoch, cutoff, wire)
            {
                return Err(Capsule);
            }
        }
    }
    // validate every retained station observation before source issuance
    for station in array(causal, "stationHours").ok_or(Capsule)? {
        validate_clock_field(station, "hourAt", epoch, cutoff, wire)?;
        if validate_clock_field(station, "receivedAt", epoch, cutoff, wire)? > issued {
            return Err(Capsule);
        }
    }
    // validate every source row as available before issuance and future-valid
    for row in array(source, "rows").ok_or(Capsule)? {
        validate_clock_field(row, "referenceAt", epoch, cutoff, wire)?;
        if validate_clock_field(row, "receivedAt", epoch, cutoff, wire)? > issued
            || validate_clock_field(row, "validAt", epoch, cutoff, wire)? <= issued
        {
            return Err(Capsule);
        }
    }
    // bind current-run completion to source availability
    if validate_clock_field(current, "completedAt", epoch, cutoff, wire)? > issued {
        return Err(Capsule);
    }
    Ok(())
}

/// Validate one required canonical clock field and return its epoch value.
fn validate_clock_field(
    value: &JsonValue,
    name: &str,
    epoch: i64,
    cutoff: i64,
    wire: &NativeWirePrimitives,
) -> Result<i64, NativeMaintenanceDailyTerminalValuesError> {
    let clock = text(value, name).ok_or(NativeMaintenanceDailyTerminalValuesError::Capsule)?;
    let milliseconds = wire
        .canonical_utc_milliseconds(&clock)
        .ok_or(NativeMaintenanceDailyTerminalValuesError::Capsule)?;
    // retain only the inclusive epoch-to-cutoff snapshot
    if milliseconds < epoch || milliseconds > cutoff {
        return Err(NativeMaintenanceDailyTerminalValuesError::Capsule);
    }
    Ok(milliseconds)
}

/// Derive the existing five-key archive member identity for one projected row.
fn archive_row_member_sha256(
    receipt: &AdjustmentRevisionCommitReceipt,
    payload_identity_sha256: &str,
    projection_kind: &str,
    row_index: usize,
    wire: &NativeWirePrimitives,
) -> Result<String, NativeMaintenanceDailyTerminalValuesError> {
    hash_value(
        wire,
        &object(vec![
            (
                "archiveCommitOrdinal",
                string(&receipt.archiveCommitOrdinal),
            ),
            ("payloadIdentitySha256", string(payload_identity_sha256)),
            ("projectionKind", string(projection_kind)),
            ("receiptSha256", string(&receipt.receiptSha256)),
            ("rowIndex", JsonValue::Number(row_index as f64)),
        ]),
    )
}

/// Decode one nested probability vector without decimal drift.
fn decode_probability(
    value: &JsonValue,
) -> Result<JsonValue, NativeMaintenanceDailyTerminalValuesError> {
    Ok(object(vec![
        (
            "atLeast0_1",
            JsonValue::Number(required_binary64(value, "atLeast0_1")?),
        ),
        (
            "atLeast1_0",
            JsonValue::Number(required_binary64(value, "atLeast1_0")?),
        ),
        (
            "atLeast2_5",
            JsonValue::Number(required_binary64(value, "atLeast2_5")?),
        ),
    ]))
}

/// Decode one required binary64 text field.
fn required_binary64(
    value: &JsonValue,
    name: &str,
) -> Result<f64, NativeMaintenanceDailyTerminalValuesError> {
    decode_binary64(
        &text(value, name).ok_or(NativeMaintenanceDailyTerminalValuesError::Projection)?,
    )
    .ok_or(NativeMaintenanceDailyTerminalValuesError::Projection)
}

/// Decode one nullable binary64 text field.
fn optional_binary64(
    value: &JsonValue,
    name: &str,
) -> Result<Option<f64>, NativeMaintenanceDailyTerminalValuesError> {
    match field(value, name) {
        Some(JsonValue::Null) => Ok(None),
        Some(JsonValue::String(units)) => String::from_utf16(units)
            .ok()
            .and_then(|value| decode_binary64(&value))
            .map(Some)
            .ok_or(NativeMaintenanceDailyTerminalValuesError::Projection),
        _ => Err(NativeMaintenanceDailyTerminalValuesError::Projection),
    }
}

/// Parse one preregistered rain cycle, operational lead, and row selector.
fn parse_rain_expected_key(value: &str) -> Option<(String, usize, usize)> {
    let (prefix, metric) = value.rsplit_once('/')?;
    // retain only the closed rain metric spelling
    if metric != "rain" {
        return None;
    }
    let (due_key, lead) = prefix.rsplit_once('/')?;
    let lead_hours = lead.parse::<usize>().ok()?;
    // retain the exact 23 operational leads
    if !(1..=23).contains(&lead_hours)
        || !due_key.starts_with("capture/")
        || due_key.len() != "capture/2026-10-11T12:35:00.000Z".len()
        || !due_key.ends_with(":35:00.000Z")
    {
        return None;
    }
    Some((due_key.to_owned(), lead_hours - 1, lead_hours))
}

/// Return the exact generated receipt projection-kind spelling.
fn receipt_kind(value: AdjustmentRevisionCommitReceiptProjectionKindEnum) -> &'static str {
    match value {
        AdjustmentRevisionCommitReceiptProjectionKindEnum::ActualBestMatch => "actual_best_match",
        AdjustmentRevisionCommitReceiptProjectionKindEnum::NativeSource => "native_source",
        AdjustmentRevisionCommitReceiptProjectionKindEnum::RainGateInput => "rain_gate_input",
        AdjustmentRevisionCommitReceiptProjectionKindEnum::ShadowPrediction => "shadow_prediction",
        AdjustmentRevisionCommitReceiptProjectionKindEnum::TargetRevision => "target_revision",
    }
}

/// Read one exact array field.
fn array<'a>(value: &'a JsonValue, name: &str) -> Option<&'a [JsonValue]> {
    let JsonValue::Array(values) = field(value, name)? else {
        return None;
    };
    Some(values)
}

/// Read one exact array item without cloning its document.
fn array_item<'a>(value: &'a JsonValue, name: &str, index: usize) -> Option<&'a JsonValue> {
    array(value, name)?.get(index)
}

/// Read one nested field.
fn nested_field<'a>(value: &'a JsonValue, outer: &str, inner: &str) -> Option<&'a JsonValue> {
    field(field(value, outer)?, inner)
}

/// Compare two required text fields.
fn same_text(left: &JsonValue, left_name: &str, right: &JsonValue, right_name: &str) -> bool {
    text(left, left_name)
        .zip(text(right, right_name))
        .is_some_and(|(left, right)| left == right)
}

/// Read one safe unsigned JSON integer.
fn integer(value: &JsonValue, name: &str) -> Option<usize> {
    integer_value(field(value, name))
}

/// Decode one optional safe unsigned JSON integer value.
fn integer_value(value: Option<&JsonValue>) -> Option<usize> {
    match value? {
        JsonValue::Number(number)
            if number.is_finite()
                && *number >= 0.0
                && number.fract() == 0.0
                && *number <= usize::MAX as f64 =>
        {
            Some(*number as usize)
        }
        _ => None,
    }
}

/// Sum one required safe integer over an ordered metadata group.
fn sum_integer(
    values: &[&JsonValue],
    name: &str,
) -> Result<usize, NativeMaintenanceDailyTerminalValuesError> {
    values.iter().try_fold(0_usize, |sum, value| {
        sum.checked_add(
            integer(value, name).ok_or(NativeMaintenanceDailyTerminalValuesError::Chunk)?,
        )
        .ok_or(NativeMaintenanceDailyTerminalValuesError::Chunk)
    })
}

/// Collect one required text field over an ordered metadata group.
fn texts(
    values: &[&JsonValue],
    name: &str,
) -> Result<Vec<String>, NativeMaintenanceDailyTerminalValuesError> {
    values
        .iter()
        .map(|value| text(value, name).ok_or(NativeMaintenanceDailyTerminalValuesError::Chunk))
        .collect()
}

/// Decode one nullable text field.
fn optional_text(
    value: &JsonValue,
    name: &str,
) -> Result<Option<String>, NativeMaintenanceDailyTerminalValuesError> {
    match field(value, name) {
        Some(JsonValue::Null) => Ok(None),
        Some(JsonValue::String(units)) => String::from_utf16(units)
            .map(Some)
            .map_err(|_| NativeMaintenanceDailyTerminalValuesError::Projection),
        _ => Err(NativeMaintenanceDailyTerminalValuesError::Projection),
    }
}

/// Decode one nullable source clock field.
fn nullable_text(
    value: &JsonValue,
    name: &str,
) -> Result<Option<String>, NativeMaintenanceDailyTerminalValuesError> {
    optional_text(value, name).map_err(|_| NativeMaintenanceDailyTerminalValuesError::Capsule)
}

/// Compare canonical positive decimal ordinals without truncation.
fn decimal_cmp(left: &str, right: &str) -> Ordering {
    left.len().cmp(&right.len()).then(left.cmp(right))
}

/// Require one positive bounded decimal archive ordinal.
fn valid_decimal(value: &str) -> bool {
    (1..=20).contains(&value.len())
        && value.as_bytes().first().is_some_and(|byte| *byte != b'0')
        && value.bytes().all(|byte| byte.is_ascii_digit())
}

/// Compare one canonical clock to the inclusive epoch-to-cutoff snapshot.
fn outside_snapshot(value: &str, epoch: i64, cutoff: i64, wire: &NativeWirePrimitives) -> bool {
    wire.canonical_utc_milliseconds(value)
        .is_none_or(|value| value < epoch || value > cutoff)
}

/// Decode exactly sixteen lowercase hexadecimal digits into one finite binary64.
fn decode_binary64(value: &str) -> Option<f64> {
    // reject alternate casing, width, and non-finite encodings
    if value.len() != 16
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return None;
    }
    let decoded = f64::from_bits(u64::from_str_radix(value, 16).ok()?);
    decoded.is_finite().then_some(decoded)
}

/// Hash one exact ordered string array.
fn hash_string_array(
    wire: &NativeWirePrimitives,
    values: &[String],
) -> Result<String, NativeMaintenanceDailyTerminalValuesError> {
    hash_value(
        wire,
        &JsonValue::Array(values.iter().map(|value| string(value)).collect()),
    )
}

/// Hash one canonical LF-framed source value.
fn hash_value(
    wire: &NativeWirePrimitives,
    value: &JsonValue,
) -> Result<String, NativeMaintenanceDailyTerminalValuesError> {
    Ok(format_digest(wire.sha256(&canonical_bytes(wire, value)?)))
}

/// Encode one exact canonical source value.
fn canonical_bytes(
    wire: &NativeWirePrimitives,
    value: &JsonValue,
) -> Result<Vec<u8>, NativeMaintenanceDailyTerminalValuesError> {
    wire.encode_sorted_json(value)
        .ok_or(NativeMaintenanceDailyTerminalValuesError::Codec)
}

/// Remove one self-authenticating field from a source object.
fn without_field(value: &JsonValue, excluded: &str) -> Option<JsonValue> {
    let JsonValue::Object(fields) = value else {
        return None;
    };
    Some(JsonValue::Object(
        fields
            .iter()
            .filter(|(name, _)| !name.iter().copied().eq(excluded.encode_utf16()))
            .cloned()
            .collect(),
    ))
}

/// Add one computed identity to an already closed object.
fn with_field(value: &JsonValue, name: &str, field_value: JsonValue) -> Option<JsonValue> {
    let JsonValue::Object(fields) = value else {
        return None;
    };
    let mut fields = fields.clone();
    fields.push((name.encode_utf16().collect(), field_value));
    Some(JsonValue::Object(fields))
}

/// Convert one local date to a timezone-independent civil day index.
fn date_day(date: &str, wire: &NativeWirePrimitives) -> Option<i64> {
    wire.canonical_utc_milliseconds(&format!("{date}T00:00:00.000Z"))
        .map(|milliseconds| milliseconds.div_euclid(DAY))
}

/// Format one civil day with the source UTC date codec.
fn format_day(day: i64, wire: &NativeWirePrimitives) -> Option<String> {
    let instant = wire.format_utc_milliseconds(day.checked_mul(DAY)?)?;
    instant.get(..10).map(str::to_owned)
}

#[cfg(test)]
#[path = "native_maintenance_daily_rain_terminal_values_test.rs"]
mod tests;
