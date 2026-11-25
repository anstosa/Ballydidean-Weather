//! Post-burn temperature and wind confirmation value assembly from verified cold history.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::{self, Display, Formatter};

use weather_contracts::{
    AdjustmentRevisionCommitReceipt, AdjustmentRevisionCommitReceiptProjectionKindEnum,
    FieldPresence, ForecastAdjustmentMetric, ForecastObservationStationKey, JsonValue,
    retained_forecast_observation,
};
use weather_edge::{
    ArchiveWirePrimitives, PrivateShadowArchiveCodec, PrivateShadowCapsuleDocument,
    PrivateShadowReceiptCodec, RevisionArchiveCanonicalCodec,
};

use crate::maintenance_canonical_json::{field, text, valid_hash};
use crate::native_maintenance_policy_comparison::{object, string};
use crate::native_maintenance_retirement_files_v2::format_digest;
use crate::{
    NativeAdjustmentRollingShadowRegistration, NativeMaintenanceCaptureFamily,
    NativeMaintenanceDailyTerminalPreparation, NativeMaintenanceDailyTerminalPreparationInput,
    NativeMaintenanceDue, NativeMaintenanceHistoricalArchiveIndex,
    NativeMaintenanceHistoricalOccurrence, NativeModelEvidenceValues,
    NativePredecessorModelCalendar, NativeShadowProducerCodec, NativeWirePrimitives,
    VerifiedNativeMaintenanceCaptureEpochWitness, decode_native_api_revision_document,
    prepare_native_maintenance_daily_terminal_material,
};

const HOUR: i64 = 3_600_000;
const DAY: i64 = 24 * HOUR;
const CYCLE: i64 = 6 * HOUR;
const MAXIMUM_PART_BYTES: usize = 8 * 1_024 * 1_024;
const SHADOW_V2: &str = "adjustment-shadow-revision-capsule/v2";
const PART_VERSION: &str = "adjustment-confirmation-chunk-part/v3";
const KNOWN_PROJECTION_CONTRACTS: &[&str] = &[
    "adjustment-revision-projection/v1",
    "adjustment-revision-batch-projection/v2",
    "adjustment-temperature-native-source-projection/v2",
    "adjustment-rain-gate-feature-projection/v2",
    "adjustment-rain-gate-control-projection/v3",
];

/// Select whether missing post-burn members remain retryable or become terminal unsupported cells.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceDailyTerminalAssemblyMode {
    Complete,
    Unsupported,
}

/// Complete source inputs after the owner has durably burned the exact blinded plan.
pub struct NativeMaintenanceDailyTerminalValuesInput<'a> {
    pub access: &'a JsonValue,
    pub catalog_input_manifest_sha256: &'a str,
    pub clock_at: &'a str,
    pub confirmation_registration: &'a JsonValue,
    pub due: &'a NativeMaintenanceDue,
    pub epoch_witness: &'a JsonValue,
    pub history: &'a NativeMaintenanceHistoricalArchiveIndex,
    pub plan: &'a JsonValue,
    pub shadow_registration: &'a NativeAdjustmentRollingShadowRegistration,
    pub source_commit: &'a str,
    pub verified_epoch_witness: &'a VerifiedNativeMaintenanceCaptureEpochWitness,
}

/// One exact immutable physical part ready for journal metadata append and later evaluator replay.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceDailyTerminalChunk {
    pub metadata: JsonValue,
    pub payload_base64: String,
}

/// One newly derived immutable regional target member and its content address.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceDailyDerivedTarget {
    pub bytes: Vec<u8>,
    pub target_member_sha256: String,
}

/// Post-burn material that is either complete, explicitly unsupported, or still retryable.
#[derive(Clone, Debug, PartialEq)]
pub enum NativeMaintenanceDailyTerminalValues {
    Pending {
        value: JsonValue,
    },
    Complete {
        chunks: Vec<NativeMaintenanceDailyTerminalChunk>,
        contract_version: &'static str,
        derived_targets: Vec<NativeMaintenanceDailyDerivedTarget>,
        family: NativeMaintenanceCaptureFamily,
        full_manifest: JsonValue,
        graph_manifest_sha256s: Vec<String>,
        registration_sha256: String,
        rain_gate_input_member_sha256s: Vec<String>,
        required_member_identity_sha256s: Vec<String>,
        terminal_graph: JsonValue,
    },
    Unsupported {
        chunks: Vec<NativeMaintenanceDailyTerminalChunk>,
        contract_version: &'static str,
        derived_targets: Vec<NativeMaintenanceDailyDerivedTarget>,
        family: NativeMaintenanceCaptureFamily,
        graph_manifest_sha256s: Vec<String>,
        missing_key_count: usize,
        missing_key_set_sha256: String,
        registration_sha256: String,
        rain_gate_input_member_sha256s: Vec<String>,
        required_member_identity_sha256s: Vec<String>,
        terminal_graph: JsonValue,
    },
}

impl NativeMaintenanceDailyTerminalValues {
    /// Return the exact predecessor terminal state spelling.
    pub const fn state(&self) -> &'static str {
        // keep unsupported evidence disjoint from the qualified complete surface
        match self {
            Self::Pending { .. } => "pending",
            Self::Complete { .. } => "burned_complete",
            Self::Unsupported { .. } => "burned_unsupported",
        }
    }
}

/// Closed assembly rejection after the terminal plan has been burned.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceDailyTerminalValuesError {
    Input,
    Plan,
    Access,
    History,
    Capsule,
    Projection,
    Target,
    Chunk,
    Codec,
}

impl Display for NativeMaintenanceDailyTerminalValuesError {
    /// Render one bounded failure without reflecting private archive values.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        // retain the owning source phase for operational diagnosis
        formatter.write_str(match self {
            Self::Input => "daily terminal value input is invalid",
            Self::Plan => "daily terminal value plan drifted",
            Self::Access => "daily terminal burn access differs",
            Self::History => "daily terminal history differs",
            Self::Capsule => "daily terminal capsule differs",
            Self::Projection => "daily terminal projection differs",
            Self::Target => "daily terminal derived target differs",
            Self::Chunk => "daily terminal chunk differs",
            Self::Codec => "daily terminal value serialization failed",
        })
    }
}

impl std::error::Error for NativeMaintenanceDailyTerminalValuesError {}

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
struct IndexedCapsule {
    body: JsonValue,
    capsule: PrivateShadowCapsuleDocument,
    comparator: JsonValue,
    comparator_bytes: Vec<u8>,
    graph_manifest_sha256: String,
    payload_bytes: Vec<u8>,
    payload_identity_sha256: String,
    source: JsonValue,
}

#[derive(Clone)]
struct IndexedProjection {
    document: JsonValue,
    graph_manifest_sha256: String,
    member_sha256: String,
    payload_identity_sha256: String,
    receipt: AdjustmentRevisionCommitReceipt,
    row: JsonValue,
    row_index: usize,
}

struct HistoryIndexes {
    actual_best_match: HashMap<String, IndexedProjection>,
    capsules: HashMap<String, IndexedCapsule>,
    native_source: HashMap<String, IndexedProjection>,
    targets_by_valid_at: HashMap<String, Vec<IndexedProjection>>,
}

struct ResolvedRecord {
    derived_target: NativeMaintenanceDailyDerivedTarget,
    graph_binding: JsonValue,
    graph_manifest_sha256s: Vec<String>,
    record: JsonValue,
    required_member_identity_sha256s: Vec<String>,
}

struct DerivedTargetResult {
    member: NativeMaintenanceDailyDerivedTarget,
    graph_manifest_sha256s: Vec<String>,
    required_member_identity_sha256s: Vec<String>,
    source_member_sha256s: Vec<String>,
    target: f64,
    value: JsonValue,
}

struct Lineage {
    adapter: &'static str,
    end: Option<&'static str>,
    fingerprint: &'static str,
    source_key: &'static str,
    start: Option<&'static str>,
    station: ForecastObservationStationKey,
}

/// Assemble the exact source chunks and terminal graph without performing journal or archive effects.
pub fn assemble_native_maintenance_daily_terminal_values(
    input: NativeMaintenanceDailyTerminalValuesInput<'_>,
    mode: NativeMaintenanceDailyTerminalAssemblyMode,
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
    let current_plan = match (&preparation, mode) {
        (NativeMaintenanceDailyTerminalPreparation::Supported { plan }, _) => plan,
        (
            NativeMaintenanceDailyTerminalPreparation::Unsupported { plan },
            NativeMaintenanceDailyTerminalAssemblyMode::Unsupported,
        ) => plan,
        _ => return Err(Error::Plan),
    };
    // bind value access to the exact plan that was frozen and burned before this call
    if canonical_bytes(&wire, current_plan)? != canonical_bytes(&wire, input.plan)? {
        return Err(Error::Plan);
    }
    let family =
        parse_family(&text(input.confirmation_registration, "family").ok_or(Error::Input)?)
            .ok_or(Error::Input)?;
    // retain rain through its separately owned fixed-gauge terminal assembler
    if family == NativeMaintenanceCaptureFamily::Rain {
        return Err(Error::Input);
    }
    let registration_sha256 =
        text(input.confirmation_registration, "registrationSha256").ok_or(Error::Input)?;
    let shadow_registration_sha256 = text(
        &input.shadow_registration.registration,
        "registrationSha256",
    )
    .ok_or(Error::Input)?;
    validate_access(
        input.access,
        input.plan,
        input.confirmation_registration,
        &wire,
    )?;
    let parts = expected_parts(
        family,
        &text(&input.shadow_registration.registration, "intervalStartAt").ok_or(Error::Input)?,
        &text(&input.shadow_registration.registration, "intervalEndAt").ok_or(Error::Input)?,
        &wire,
    )?;
    let cutoff = text(input.plan, "targetCutoffAt").ok_or(Error::Plan)?;
    let indexes = build_history_indexes(
        family,
        input.history,
        &shadow_registration_sha256,
        input.verified_epoch_witness,
        &cutoff,
        &wire,
    )?;
    let mut target_cache = HashMap::<String, Option<DerivedTargetResult>>::new();
    let mut chunks = Vec::with_capacity(parts.len());
    let mut derived_targets = BTreeMap::<String, NativeMaintenanceDailyDerivedTarget>::new();
    let mut graph_bindings = Vec::new();
    let mut graph_manifest_sha256s = BTreeSet::new();
    let mut missing_keys = Vec::new();
    let mut required_member_identity_sha256s = BTreeSet::new();
    // preserve every preregistered physical part independently from value availability
    for part in &parts {
        let mut records = Vec::new();
        let mut part_missing_keys = Vec::new();
        // resolve each expected cell against only authenticated historical members
        for key in &part.expected_keys {
            let (due_key, row_index, metric) =
                parse_expected_key(key, family).ok_or(Error::Input)?;
            let resolved = if let Some(capsule) = indexes.capsules.get(&due_key) {
                resolve_record(
                    family,
                    key,
                    metric.as_deref(),
                    row_index,
                    capsule,
                    &indexes,
                    input.verified_epoch_witness,
                    &cutoff,
                    &mut target_cache,
                    &wire,
                )?
            } else {
                None
            };
            // complete assembly remains retryable rather than relabelling missing evidence
            let Some(resolved) = resolved else {
                if mode == NativeMaintenanceDailyTerminalAssemblyMode::Complete {
                    return Ok(NativeMaintenanceDailyTerminalValues::Pending {
                        value: pending_value(family, &registration_sha256),
                    });
                }
                part_missing_keys.push(key.clone());
                missing_keys.push(key.clone());
                continue;
            };
            derived_targets.insert(
                resolved.derived_target.target_member_sha256.clone(),
                resolved.derived_target,
            );
            graph_bindings.push(resolved.graph_binding);
            graph_manifest_sha256s.extend(resolved.graph_manifest_sha256s);
            required_member_identity_sha256s.extend(resolved.required_member_identity_sha256s);
            records.push(resolved.record);
        }
        chunks.push(build_chunk(
            part,
            family,
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
    let derived_targets: Vec<_> = derived_targets.into_values().collect();
    // unsupported assembly records the complete missing cell set without qualified material
    if mode == NativeMaintenanceDailyTerminalAssemblyMode::Unsupported {
        let missing_values = JsonValue::Array(missing_keys.iter().map(|key| string(key)).collect());
        let missing_key_set_sha256 = hash_value(&wire, &missing_values)?;
        let terminal_graph = terminal_graph(
            "adjustment-maintenance-confirmation-unsupported-terminal-graph/v1",
            graph_bindings,
            &derived_targets,
            &graph_manifest_sha256s,
            Some(missing_key_set_sha256.as_str()),
            &wire,
        )?;
        return Ok(NativeMaintenanceDailyTerminalValues::Unsupported {
            chunks,
            contract_version: "adjustment-maintenance-unsupported-confirmation-values/v1",
            derived_targets,
            family,
            graph_manifest_sha256s,
            missing_key_count: missing_keys.len(),
            missing_key_set_sha256,
            registration_sha256,
            rain_gate_input_member_sha256s: Vec::new(),
            required_member_identity_sha256s,
            terminal_graph,
        });
    }
    let metadata: Vec<_> = chunks.iter().map(|chunk| chunk.metadata.clone()).collect();
    let full_manifest = assemble_manifest(
        input.access,
        input.confirmation_registration,
        &metadata,
        family,
        &wire,
    )?;
    let terminal_graph = terminal_graph(
        "adjustment-maintenance-confirmation-terminal-graph/v1",
        graph_bindings,
        &derived_targets,
        &graph_manifest_sha256s,
        None,
        &wire,
    )?;
    Ok(NativeMaintenanceDailyTerminalValues::Complete {
        chunks,
        contract_version: "adjustment-maintenance-confirmation-values/v1",
        derived_targets,
        family,
        full_manifest,
        graph_manifest_sha256s,
        registration_sha256,
        rain_gate_input_member_sha256s: Vec::new(),
        required_member_identity_sha256s,
        terminal_graph,
    })
}

/// Recreate every exact physical key part from the frozen provider cadence.
fn expected_parts(
    family: NativeMaintenanceCaptureFamily,
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
    let (logical_count, interval_days, first_lead, row_count) = match family {
        NativeMaintenanceCaptureFamily::Temperature => (27_usize, 366_i64, 1_i64, 12_i64),
        NativeMaintenanceCaptureFamily::Wind => (27, 366, 1, 168),
        NativeMaintenanceCaptureFamily::Rain => return Err(Input),
    };
    // require the reviewed local-date interval before enumerating source cycles
    if end <= start || end_day - start_day != interval_days {
        return Err(Input);
    }
    let maximum_lead = first_lead + row_count - 1;
    let first_cycle = start
        .checked_sub(maximum_lead * HOUR)
        .ok_or(Input)?
        .div_euclid(CYCLE)
        * CYCLE;
    let mut groups = BTreeMap::<(usize, String), Vec<String>>::new();
    let mut cycle = first_cycle;
    // enumerate fixed source cycles without consulting any observed outcome
    while cycle < end {
        let due_at = wire
            .format_utc_milliseconds(cycle.checked_add(35 * 60_000).ok_or(Input)?)
            .ok_or(Input)?;
        let due_key = format!("capture/{due_at}");
        let capture_local_date = calendar.features(&due_at).map_err(|_| Input)?.local_date;
        // retain each supported row and wind metric independently
        for row in 0..row_count {
            let model_lead = first_lead + row;
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
            let keys = groups
                .entry((logical_index, capture_local_date.clone()))
                .or_default();
            let lead = row + 1;
            match family {
                NativeMaintenanceCaptureFamily::Temperature => {
                    keys.push(format!("{due_key}/{lead}/temperature"));
                }
                NativeMaintenanceCaptureFamily::Wind => {
                    keys.push(format!("{due_key}/{lead}/windSpeedMps"));
                    // preserve the frozen unsupported gust band
                    if !(49..=72).contains(&model_lead) {
                        keys.push(format!("{due_key}/{lead}/windGustMps"));
                    }
                }
                NativeMaintenanceCaptureFamily::Rain => return Err(Input),
            }
        }
        cycle = cycle.checked_add(CYCLE).ok_or(Input)?;
    }
    let mut parts = Vec::new();
    // materialize deterministic target-window and capture-date parts
    for logical_index in 0..logical_count {
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

/// Build one exact deduplicated v3 physical part and its journal metadata.
fn build_chunk(
    part: &ExpectedPart,
    family: NativeMaintenanceCaptureFamily,
    confirmation_registration_sha256: &str,
    shadow_registration_sha256: &str,
    records: Vec<JsonValue>,
    missing_keys: Vec<String>,
    wire: &NativeWirePrimitives,
) -> Result<NativeMaintenanceDailyTerminalChunk, NativeMaintenanceDailyTerminalValuesError> {
    use NativeMaintenanceDailyTerminalValuesError::Chunk;

    let mut capsules = BTreeMap::<String, JsonValue>::new();
    let mut projected_records = Vec::with_capacity(records.len());
    // deduplicate exact capsule bytes while preserving expected-key record order
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
        let comparison = field(&record, "comparison").cloned().ok_or(Chunk)?;
        let key = text(&record, "key").ok_or(Chunk)?;
        let metric = field(&record, "metric").cloned().ok_or(Chunk)?;
        let row_index = field(&record, "rowIndex").cloned().ok_or(Chunk)?;
        projected_records.push(object(vec![
            ("capsuleMemberSha256", string(&capsule_member_sha256)),
            ("comparison", comparison),
            ("key", string(&key)),
            ("metric", metric),
            ("rowIndex", row_index),
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
    // prove the eligible and missing sets are disjoint and equal the preregistration
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
        ("family", string(family.wire())),
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
    // preserve the source's bounded physical archive member ceiling
    if payload_bytes.len() > MAXIMUM_PART_BYTES {
        return Err(Chunk);
    }
    let metadata = object(vec![
        ("captureLocalDate", string(&part.capture_local_date)),
        ("contractVersion", string(PART_VERSION)),
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

/// Assemble the exact logical v3 full-member identity from ordered physical metadata.
fn assemble_manifest(
    access: &JsonValue,
    registration: &JsonValue,
    parts: &[JsonValue],
    family: NativeMaintenanceCaptureFamily,
    wire: &NativeWirePrimitives,
) -> Result<JsonValue, NativeMaintenanceDailyTerminalValuesError> {
    use NativeMaintenanceDailyTerminalValuesError::Chunk;

    let logical_count = match family {
        NativeMaintenanceCaptureFamily::Temperature | NativeMaintenanceCaptureFamily::Wind => 27,
        NativeMaintenanceCaptureFamily::Rain => return Err(Chunk),
    };
    let mut logical_chunks = Vec::with_capacity(logical_count);
    let mut expected_roots = Vec::with_capacity(logical_count);
    // fold consecutive physical parts into the frozen logical window population
    for logical_index in 0..logical_count {
        let group: Vec<_> = parts
            .iter()
            .filter(|part| integer(part, "logicalChunkIndex") == Some(logical_index))
            .collect();
        // require one complete ordered part sequence for this logical window
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
        ("chunkCount", JsonValue::Number(logical_count as f64)),
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
        ("family", string(family.wire())),
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

/// Authenticate and index the exact capsule, source, comparator, and target archive population.
fn build_history_indexes(
    family: NativeMaintenanceCaptureFamily,
    history: &NativeMaintenanceHistoricalArchiveIndex,
    registration_sha256: &str,
    witness: &VerifiedNativeMaintenanceCaptureEpochWitness,
    target_cutoff_at: &str,
    wire: &NativeWirePrimitives,
) -> Result<HistoryIndexes, NativeMaintenanceDailyTerminalValuesError> {
    use NativeMaintenanceDailyTerminalValuesError::{History, Projection};

    let epoch = wire
        .canonical_utc_milliseconds(&witness.epoch_at)
        .ok_or(History)?;
    let cutoff = wire
        .canonical_utc_milliseconds(target_cutoff_at)
        .ok_or(History)?;
    let codec = NativeShadowProducerCodec::new(NativeModelEvidenceValues::new());
    let mut indexes = HistoryIndexes {
        actual_best_match: HashMap::new(),
        capsules: HashMap::new(),
        native_source: HashMap::new(),
        targets_by_valid_at: HashMap::new(),
    };
    // decode only published members from the independently verified historical prefix
    for occurrence in &history.occurrences {
        if occurrence.publication_disposition != "published" {
            continue;
        }
        validate_occurrence_envelope(occurrence, wire)?;
        // retain same-registration capsules through their distinct private codec
        if occurrence.payload_kind == SHADOW_V2 {
            let capsule = PrivateShadowReceiptCodec::decode_capsule_document(
                &codec,
                &occurrence.payload_bytes,
            )
            .ok_or(Projection)?;
            if !PrivateShadowArchiveCodec::admit_stage_input(&codec, &capsule.stage_input) {
                return Err(Projection);
            }
            // omit other registrations only after complete capsule admission
            if capsule.stage_input.metadata.registrationSha256 != registration_sha256 {
                continue;
            }
            let body = wire
                .decode_json(&capsule.stage_input.body)
                .ok_or(Projection)?;
            let source = wire
                .decode_json(&capsule.stage_input.sourceProjection)
                .ok_or(Projection)?;
            let comparator_bytes = match &capsule.stage_input.comparator {
                FieldPresence::Value(bytes) => bytes.clone(),
                FieldPresence::Missing => return Err(Projection),
            };
            let comparator = wire.decode_json(&comparator_bytes).ok_or(Projection)?;
            let prediction_committed_at = match &capsule.prediction_committed_at {
                FieldPresence::Value(value) => value,
                FieldPresence::Missing => return Err(Projection),
            };
            let receipt = occurrence.receipts.first().ok_or(Projection)?;
            // bind the sole receipt and all post-epoch cutoff clocks
            if occurrence.receipts.len() != 1
                || receipt != &capsule.revision_receipt
                || outside_snapshot(&receipt.archiveCommittedAt, epoch, cutoff, wire)
                || outside_snapshot(prediction_committed_at, epoch, cutoff, wire)
            {
                return Err(Projection);
            }
            let due_key = capsule.stage_input.metadata.dueKey.clone();
            if indexes
                .capsules
                .insert(
                    due_key,
                    IndexedCapsule {
                        body,
                        capsule,
                        comparator,
                        comparator_bytes,
                        graph_manifest_sha256: occurrence.graph_manifest_sha256.clone(),
                        payload_bytes: occurrence.payload_bytes.clone(),
                        payload_identity_sha256: occurrence.payload_identity_sha256.clone(),
                        source,
                    },
                )
                .is_some()
            {
                return Err(Projection);
            }
            continue;
        }
        // ignore only explicitly disjoint contracts without opening their values
        if !KNOWN_PROJECTION_CONTRACTS.contains(&occurrence.payload_kind.as_str()) {
            continue;
        }
        decode_native_api_revision_document(&occurrence.payload_bytes).ok_or(Projection)?;
        let document = wire
            .decode_json(&occurrence.payload_bytes)
            .ok_or(Projection)?;
        let projection_kind = text(&document, "projectionKind").ok_or(Projection)?;
        let contract = text(&document, "contractVersion").ok_or(Projection)?;
        let JsonValue::Array(rows) = field(&document, "rows").ok_or(Projection)? else {
            return Err(Projection);
        };
        let expected_receipts = if contract == "adjustment-revision-batch-projection/v2" {
            rows.len()
        } else {
            1
        };
        // preserve exact single or per-row receipt population ownership
        if occurrence.payload_kind != contract || occurrence.receipts.len() != expected_receipts {
            return Err(Projection);
        }
        validate_document_clocks(&document, epoch, cutoff, wire)?;
        for (row_index, row) in rows.iter().enumerate() {
            let receipt = if occurrence.receipts.len() == 1 {
                occurrence.receipts.first()
            } else {
                occurrence.receipts.get(row_index)
            }
            .ok_or(Projection)?;
            // bind each row to its exact projection receipt and snapshot clocks
            if receipt_kind(receipt.projectionKind) != projection_kind
                || receipt.projectionIdentitySha256 != occurrence.payload_identity_sha256
                || receipt.projectionSha256 != occurrence.payload_identity_sha256
                || outside_snapshot(
                    &text(row, "validAt").ok_or(Projection)?,
                    epoch,
                    cutoff,
                    wire,
                )
                || optional_or_missing_text(row, "bestMatchProductRunAt")?
                    .is_some_and(|clock| outside_snapshot(&clock, epoch, cutoff, wire))
                || outside_snapshot(&receipt.archiveCommittedAt, epoch, cutoff, wire)
            {
                return Err(Projection);
            }
            let member_identity = object(vec![
                (
                    "archiveCommitOrdinal",
                    string(&receipt.archiveCommitOrdinal),
                ),
                (
                    "payloadIdentitySha256",
                    string(&occurrence.payload_identity_sha256),
                ),
                ("projectionKind", string(&projection_kind)),
                ("receiptSha256", string(&receipt.receiptSha256)),
                ("rowIndex", JsonValue::Number(row_index as f64)),
            ]);
            let member = IndexedProjection {
                document: document.clone(),
                graph_manifest_sha256: occurrence.graph_manifest_sha256.clone(),
                member_sha256: hash_value(wire, &member_identity)?,
                payload_identity_sha256: occurrence.payload_identity_sha256.clone(),
                receipt: receipt.clone(),
                row: row.clone(),
                row_index,
            };
            match projection_kind.as_str() {
                "actual_best_match" => {
                    let key = if family == NativeMaintenanceCaptureFamily::Temperature {
                        temperature_comparator_identity(&member, wire)?
                    } else {
                        text(&member.row, "contentSha256").ok_or(Projection)?
                    };
                    insert_unique(&mut indexes.actual_best_match, key, member)?;
                }
                "native_source" => {
                    // temperature excludes unrelated native forecast anchor bodies
                    if family == NativeMaintenanceCaptureFamily::Temperature
                        && text(&document, "family").as_deref() != Some("temperature")
                    {
                        continue;
                    }
                    if family == NativeMaintenanceCaptureFamily::Temperature {
                        let key = temperature_native_identity(&member, wire)?;
                        insert_unique(&mut indexes.native_source, key, member)?;
                    }
                }
                "target_revision" => {
                    let valid_at = text(&member.row, "validAt").ok_or(Projection)?;
                    indexes
                        .targets_by_valid_at
                        .entry(valid_at)
                        .or_default()
                        .push(member);
                }
                _ => {}
            }
        }
    }
    Ok(indexes)
}

/// Resolve one exact expected cell into its comparator, native, and derived-target record.
#[allow(clippy::too_many_arguments)]
fn resolve_record(
    family: NativeMaintenanceCaptureFamily,
    key: &str,
    metric: Option<&str>,
    row_index: usize,
    capsule: &IndexedCapsule,
    indexes: &HistoryIndexes,
    witness: &VerifiedNativeMaintenanceCaptureEpochWitness,
    target_cutoff_at: &str,
    target_cache: &mut HashMap<String, Option<DerivedTargetResult>>,
    wire: &NativeWirePrimitives,
) -> Result<Option<ResolvedRecord>, NativeMaintenanceDailyTerminalValuesError> {
    use NativeMaintenanceDailyTerminalValuesError::{Capsule, Projection};

    let body_family = text(&capsule.body, "family").ok_or(Capsule)?;
    let source_family = text(&capsule.source, "family").ok_or(Capsule)?;
    let due_key = capsule.capsule.stage_input.metadata.dueKey.as_str();
    // reject cross-family bodies and inconsistent private due bindings
    if body_family != family.wire()
        || source_family != family.wire()
        || text(&capsule.body, "dueKey").as_deref() != Some(due_key)
    {
        return Ok(None);
    }
    let body_row = array_item(&capsule.body, "rows", row_index).ok_or(Capsule)?;
    let source_row = array_item(&capsule.source, "rows", row_index).ok_or(Capsule)?;
    let comparator_row = array_item(&capsule.comparator, "rows", row_index).ok_or(Capsule)?;
    let expected_lead = row_index + 1;
    let valid_at = text(body_row, "validAt").ok_or(Capsule)?;
    // bind row geometry and all candidate-consumed clocks before reading model values
    if integer(body_row, "leadHours") != Some(expected_lead)
        || integer(source_row, "leadHours") != Some(expected_lead)
        || integer(comparator_row, "leadHours") != Some(expected_lead)
        || text(source_row, "validAt").as_deref() != Some(valid_at.as_str())
        || text(comparator_row, "validAt").as_deref() != Some(valid_at.as_str())
    {
        return Ok(None);
    }
    let epoch = wire
        .canonical_utc_milliseconds(&witness.epoch_at)
        .ok_or(Capsule)?;
    let cutoff = wire
        .canonical_utc_milliseconds(target_cutoff_at)
        .ok_or(Capsule)?;
    for clock in [
        text(&capsule.source, "issuedAt").ok_or(Capsule)?,
        text(source_row, "receivedAt").ok_or(Capsule)?,
        text(source_row, "referenceAt").ok_or(Capsule)?,
        valid_at.clone(),
    ] {
        if outside_snapshot(&clock, epoch, cutoff, wire) {
            return Ok(None);
        }
    }
    let (native_member, actual_member, incumbent_prediction) = match family {
        NativeMaintenanceCaptureFamily::Temperature => {
            let native_key = hash_value(
                wire,
                &object(vec![
                    (
                        "contentSha256",
                        string(&text(source_row, "contentSha256").ok_or(Capsule)?),
                    ),
                    (
                        "providerResponseSha256",
                        string(&text(source_row, "providerResponseSha256").ok_or(Capsule)?),
                    ),
                    (
                        "runInitializedAt",
                        string(&text(source_row, "referenceAt").ok_or(Capsule)?),
                    ),
                    ("validAt", string(&valid_at)),
                ]),
            )?;
            let actual_key = hash_value(
                wire,
                &object(vec![
                    (
                        "contentSha256",
                        string(&text(source_row, "bestMatchContentSha256").ok_or(Capsule)?),
                    ),
                    (
                        "productRunAt",
                        string(&text(source_row, "bestMatchProductRunAt").ok_or(Capsule)?),
                    ),
                    (
                        "sourceId",
                        string(&text(source_row, "bestMatchSourceId").ok_or(Capsule)?),
                    ),
                    ("validAt", string(&valid_at)),
                ]),
            )?;
            let Some(native) = indexes.native_source.get(&native_key) else {
                return Ok(None);
            };
            let Some(actual) = indexes.actual_best_match.get(&actual_key) else {
                return Ok(None);
            };
            // require the complete archived native decision rather than content-only aliases
            if !temperature_source_matches(native, actual, &capsule.source, source_row)? {
                return Ok(None);
            }
            (
                native,
                actual,
                decode_binary64(&text(comparator_row, "incumbentTemperatureC64").ok_or(Capsule)?)
                    .ok_or(Capsule)?,
            )
        }
        NativeMaintenanceCaptureFamily::Wind => {
            let content = text(source_row, "contentSha256").ok_or(Capsule)?;
            let Some(actual) = indexes.actual_best_match.get(&content) else {
                return Ok(None);
            };
            let field_name = if metric == Some("windGustMps") {
                "incumbentGustMps64"
            } else {
                "incumbentSpeedMps64"
            };
            (
                actual,
                actual,
                decode_binary64(&text(comparator_row, field_name).ok_or(Capsule)?)
                    .ok_or(Capsule)?,
            )
        }
        NativeMaintenanceCaptureFamily::Rain => return Err(Projection),
    };
    // keep only source members for the same exact target clock
    if text(&native_member.row, "validAt").as_deref() != Some(valid_at.as_str())
        || text(&actual_member.row, "validAt").as_deref() != Some(valid_at.as_str())
    {
        return Ok(None);
    }
    let metric_name = metric.unwrap_or("temperatureC");
    let cache_key = format!("{valid_at}\0{metric_name}");
    if !target_cache.contains_key(&cache_key) {
        let target = build_derived_target(
            family,
            metric_name,
            &valid_at,
            indexes
                .targets_by_valid_at
                .get(&valid_at)
                .map(Vec::as_slice)
                .unwrap_or(&[]),
            witness,
            target_cutoff_at,
            wire,
        )?;
        target_cache.insert(cache_key.clone(), target);
    }
    let Some(target) = target_cache.get(&cache_key).and_then(Option::as_ref) else {
        return Ok(None);
    };
    let prediction_committed_at = match &capsule.capsule.prediction_committed_at {
        FieldPresence::Value(value) => value,
        FieldPresence::Missing => return Err(Capsule),
    };
    let target_first_edge = text(&target.value, "firstEdgeCommittedAt").ok_or(Capsule)?;
    let first_edge_committed_at = [
        prediction_committed_at.as_str(),
        capsule.capsule.revision_receipt.archiveCommittedAt.as_str(),
        actual_member.receipt.archiveCommittedAt.as_str(),
        native_member.receipt.archiveCommittedAt.as_str(),
        target_first_edge.as_str(),
    ]
    .into_iter()
    .max()
    .ok_or(Capsule)?
    .to_owned();
    let comparator_member_sha256 = format_digest(wire.sha256(&capsule.comparator_bytes));
    let membership = object(vec![
        (
            "actualBestMatchProjectionSha256",
            string(&actual_member.payload_identity_sha256),
        ),
        (
            "actualBestMatchReceiptSha256",
            string(&actual_member.receipt.receiptSha256),
        ),
        ("incumbentMemberSha256", string(&comparator_member_sha256)),
        (
            "nativeSourceProjectionSha256",
            string(&native_member.payload_identity_sha256),
        ),
        (
            "nativeSourceReceiptSha256",
            string(&native_member.receipt.receiptSha256),
        ),
        (
            "targetMemberSha256",
            string(&target.member.target_member_sha256),
        ),
    ]);
    let comparison = object(vec![
        ("farmTarget", JsonValue::Null),
        ("firstEdgeCommittedAt", string(&first_edge_committed_at)),
        (
            "incumbentPrediction",
            JsonValue::Number(incumbent_prediction),
        ),
        ("membership", membership),
        ("nearestThree", JsonValue::Null),
        ("providerFamily", JsonValue::Null),
        ("stationKey", JsonValue::Null),
        ("target", JsonValue::Number(target.target)),
    ]);
    let serving_authority = field(&capsule.comparator, "servingAuthority").ok_or(Capsule)?;
    let mut required = BTreeSet::from([
        capsule.payload_identity_sha256.clone(),
        capsule.capsule.revision_receipt.receiptSha256.clone(),
        capsule
            .capsule
            .stage_input
            .metadata
            .predictionBodySha256
            .clone(),
        capsule.capsule.stage_input.sourceProjectionSha256.clone(),
        comparator_member_sha256.clone(),
        text(serving_authority, "receiptMemberSha256").ok_or(Capsule)?,
        actual_member.member_sha256.clone(),
        actual_member.payload_identity_sha256.clone(),
        actual_member.receipt.receiptSha256.clone(),
        native_member.member_sha256.clone(),
        native_member.payload_identity_sha256.clone(),
        native_member.receipt.receiptSha256.clone(),
        target.member.target_member_sha256.clone(),
    ]);
    required.extend(target.required_member_identity_sha256s.iter().cloned());
    // include the incumbent artifact only when serving was not the raw baseline
    if let Some(artifact) = optional_text(serving_authority, "artifactMemberSha256")? {
        required.insert(artifact);
    }
    let graph_binding = object(vec![
        ("actualBestMatch", graph_member(actual_member)),
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
                    string(&capsule.capsule.stage_input.metadata.predictionBodySha256),
                ),
                (
                    "receiptSha256",
                    string(&capsule.capsule.revision_receipt.receiptSha256),
                ),
                (
                    "sourceProjectionSha256",
                    string(&capsule.capsule.stage_input.sourceProjectionSha256),
                ),
            ]),
        ),
        ("key", string(key)),
        ("nativeSource", graph_member(native_member)),
        (
            "target",
            object(vec![
                (
                    "sourceMemberSha256s",
                    JsonValue::Array(
                        target
                            .source_member_sha256s
                            .iter()
                            .map(|identity| string(identity))
                            .collect(),
                    ),
                ),
                (
                    "targetMemberSha256",
                    string(&target.member.target_member_sha256),
                ),
            ]),
        ),
    ]);
    let mut graphs = BTreeSet::from([
        capsule.graph_manifest_sha256.clone(),
        actual_member.graph_manifest_sha256.clone(),
        native_member.graph_manifest_sha256.clone(),
    ]);
    graphs.extend(target.graph_manifest_sha256s.iter().cloned());
    Ok(Some(ResolvedRecord {
        derived_target: target.member.clone(),
        graph_binding,
        graph_manifest_sha256s: graphs.into_iter().collect(),
        record: object(vec![
            (
                "capsuleBase64",
                string(
                    &wire
                        .encode_standard_base64(&capsule.payload_bytes)
                        .ok_or(Capsule)?,
                ),
            ),
            ("comparison", comparison),
            ("key", string(key)),
            ("metric", metric.map(string).unwrap_or(JsonValue::Null)),
            ("rowIndex", JsonValue::Number(row_index as f64)),
        ]),
        required_member_identity_sha256s: required.into_iter().collect(),
    }))
}

/// Build one exact regional weighted-median target from authenticated physical rows.
fn build_derived_target(
    family: NativeMaintenanceCaptureFamily,
    metric: &str,
    valid_at: &str,
    members: &[IndexedProjection],
    witness: &VerifiedNativeMaintenanceCaptureEpochWitness,
    target_cutoff_at: &str,
    wire: &NativeWirePrimitives,
) -> Result<Option<DerivedTargetResult>, NativeMaintenanceDailyTerminalValuesError> {
    use NativeMaintenanceDailyTerminalValuesError::Target;

    let metric_field = match (family, metric) {
        (NativeMaintenanceCaptureFamily::Temperature, "temperatureC") => "temperatureC64",
        (NativeMaintenanceCaptureFamily::Wind, "windSpeedMps") => "windSpeedMps64",
        (NativeMaintenanceCaptureFamily::Wind, "windGustMps") => "windGustMps64",
        _ => return Err(Target),
    };
    let epoch = wire
        .canonical_utc_milliseconds(&witness.epoch_at)
        .ok_or(Target)?;
    let cutoff = wire
        .canonical_utc_milliseconds(target_cutoff_at)
        .ok_or(Target)?;
    let mut stations = BTreeMap::<String, (&IndexedProjection, &'static Lineage, f64)>::new();
    // select one exact eligible physical row for every admitted station lineage
    for member in members {
        if text(&member.row, "validAt").as_deref() != Some(valid_at) {
            continue;
        }
        let Some(encoded) = optional_text(&member.row, metric_field)? else {
            continue;
        };
        let source = field(&member.document, "source").ok_or(Target)?;
        let source_key = text(source, "sourceKey").ok_or(Target)?;
        let lineage = source_lineage(&source_key).ok_or(Target)?;
        validate_lineage(source, lineage, valid_at, wire)?;
        let station = retained_forecast_observation::STATIONS
            .iter()
            .find(|station| station.key == lineage.station)
            .ok_or(Target)?;
        // preserve the source local-station exclusion and metric eligibility
        if station.key == ForecastObservationStationKey::BallydideanEcowitt
            || !station
                .eligible_metrics
                .iter()
                .any(|candidate| metric_name(*candidate) == metric)
        {
            continue;
        }
        let station_key = station_name(station.key).ok_or(Target)?.to_owned();
        // refuse duplicate physical station authority instead of selecting by archive order
        if stations
            .insert(
                station_key,
                (member, lineage, decode_binary64(&encoded).ok_or(Target)?),
            )
            .is_some()
        {
            return Err(Target);
        }
        if outside_snapshot(&member.receipt.archiveCommittedAt, epoch, cutoff, wire) {
            return Err(Target);
        }
    }
    // preserve genuine insufficient regional coverage without substituting a target
    if stations.len() < 3
        || !stations.values().any(|(member, lineage, _)| {
            let _ = member;
            retained_forecast_observation::STATIONS
                .iter()
                .find(|station| station.key == lineage.station)
                .is_some_and(|station| station.nearest_rank <= 3)
        })
    {
        return Ok(None);
    }
    let mut weighted = Vec::with_capacity(stations.len());
    let mut total = 0.0;
    // normalize weights in physical-station-key lexical order
    for (station_key, (_, lineage, value)) in &stations {
        let station = retained_forecast_observation::STATIONS
            .iter()
            .find(|station| station.key == lineage.station)
            .ok_or(Target)?;
        total += station.unnormalized_spatial_weight;
        weighted.push((
            *value,
            station_key.as_str(),
            station.unnormalized_spatial_weight,
        ));
    }
    // reject nonfinite aggregate weight rather than manufacturing a regional value
    if !total.is_finite() || total <= 0.0 {
        return Err(Target);
    }
    for (_, _, weight) in &mut weighted {
        *weight /= total;
    }
    weighted.sort_by(|left, right| {
        left.0
            .partial_cmp(&right.0)
            .unwrap_or(Ordering::Equal)
            .then_with(|| left.1.cmp(right.1))
    });
    let normalized_total: f64 = weighted.iter().map(|(_, _, weight)| *weight).sum();
    let half = normalized_total / 2.0;
    let mut cumulative = 0.0;
    let mut target = None;
    // select the first strict half crossing or average an exact midpoint tie
    for (index, (value, _, weight)) in weighted.iter().enumerate() {
        cumulative += weight;
        if cumulative > half {
            target = Some(*value);
            break;
        }
        if cumulative == half {
            target = Some(
                weighted
                    .get(index + 1)
                    .map_or(*value, |next| (*value + next.0) / 2.0),
            );
            break;
        }
    }
    let target = target
        .or_else(|| weighted.last().map(|last| last.0))
        .ok_or(Target)?;
    let mut rows = Vec::with_capacity(stations.len());
    let mut graph_manifest_sha256s = BTreeSet::new();
    let mut required = BTreeSet::new();
    let mut first_edge_committed_at = None::<String>;
    // project the exact five-key row identities in source-key and member order
    for (_, (member, lineage, _)) in stations {
        first_edge_committed_at = Some(first_edge_committed_at.as_deref().map_or(
            member.receipt.archiveCommittedAt.clone(),
            |current| {
                current
                    .max(member.receipt.archiveCommittedAt.as_str())
                    .to_owned()
            },
        ));
        graph_manifest_sha256s.insert(member.graph_manifest_sha256.clone());
        required.extend([
            member.member_sha256.clone(),
            member.payload_identity_sha256.clone(),
            member.receipt.receiptSha256.clone(),
        ]);
        rows.push(object(vec![
            (
                "archiveCommitOrdinal",
                string(&member.receipt.archiveCommitOrdinal),
            ),
            ("graphManifestSha256", string(&member.graph_manifest_sha256)),
            ("memberSha256", string(&member.member_sha256)),
            (
                "payloadIdentitySha256",
                string(&member.payload_identity_sha256),
            ),
            ("receiptSha256", string(&member.receipt.receiptSha256)),
            ("rowIndex", JsonValue::Number(member.row_index as f64)),
            ("sourceKey", string(lineage.source_key)),
        ]));
    }
    rows.sort_by(|left, right| {
        text(left, "sourceKey")
            .cmp(&text(right, "sourceKey"))
            .then_with(|| text(left, "memberSha256").cmp(&text(right, "memberSha256")))
    });
    let source_member_sha256s = rows
        .iter()
        .map(|row| text(row, "memberSha256").ok_or(Target))
        .collect::<Result<Vec<_>, _>>()?;
    let value = object(vec![
        (
            "contractVersion",
            string("adjustment-maintenance-derived-target/v1"),
        ),
        ("family", string(family.wire())),
        (
            "firstEdgeCommittedAt",
            string(&first_edge_committed_at.ok_or(Target)?),
        ),
        ("metric", string(metric)),
        ("rows", JsonValue::Array(rows)),
        ("target64", string(&format!("{:016x}", target.to_bits()))),
        ("validAt", string(valid_at)),
    ]);
    let bytes = canonical_bytes(wire, &value)?;
    Ok(Some(DerivedTargetResult {
        member: NativeMaintenanceDailyDerivedTarget {
            target_member_sha256: format_digest(wire.sha256(&bytes)),
            bytes,
        },
        graph_manifest_sha256s: graph_manifest_sha256s.into_iter().collect(),
        required_member_identity_sha256s: required.into_iter().collect(),
        source_member_sha256s,
        target,
        value,
    }))
}

/// Enforce the source's complete native-temperature and comparator tuple binding.
fn temperature_source_matches(
    native: &IndexedProjection,
    actual: &IndexedProjection,
    source: &JsonValue,
    source_row: &JsonValue,
) -> Result<bool, NativeMaintenanceDailyTerminalValuesError> {
    use NativeMaintenanceDailyTerminalValuesError::Projection;

    let native_source = field(&native.document, "source").ok_or(Projection)?;
    let native_key = field(&native.document, "logicalKey").ok_or(Projection)?;
    let actual_key = field(&actual.document, "logicalKey").ok_or(Projection)?;
    let recent_state = field(&native.document, "recentErrorState").ok_or(Projection)?;
    let source_state = field(source, "recentErrorState").ok_or(Projection)?;
    let native_received = text(&native.document, "logicalReceivedAt").ok_or(Projection)?;
    let actual_received = text(&actual.document, "logicalReceivedAt").ok_or(Projection)?;
    let source_issued = text(source, "issuedAt").ok_or(Projection)?;
    Ok(text(&native.document, "contractVersion").as_deref()
        == Some("adjustment-temperature-native-source-projection/v2")
        && native_received <= source_issued
        && actual_received <= source_issued
        && text(native_key, "runInitializedAt") == text(source_row, "referenceAt")
        && text(native_key, "providerResponseSha256") == text(source_row, "providerResponseSha256")
        && text(&native.row, "modelCycle") == text(source_row, "modelCycle")
        && text(native_source, "adapterVersion") == text(source_row, "adapterVersion")
        && text(native_source, "dataset") == text(source_row, "dataset")
        && text(native_source, "providerKey") == text(source_row, "providerKey")
        && optional_text(native_source, "upstreamModel")?
            == optional_text(source_row, "upstreamModel")?
        && recent_state == source_state
        && integer(&native.row, "modelLeadHours") == integer(source_row, "modelLeadHours")
        && text(&native.row, "bestMatchContentSha256")
            == text(source_row, "bestMatchContentSha256")
        && text(&native.row, "bestMatchProductRunAt") == text(source_row, "bestMatchProductRunAt")
        && text(&native.row, "bestMatchSourceId") == text(source_row, "bestMatchSourceId")
        && text(&native.row, "bestMatchTemperatureC64")
            == text(source_row, "bestMatchTemperatureC64")
        && text(&actual.row, "temperatureC64") == text(source_row, "bestMatchTemperatureC64")
        && text(&native.row, "rawTemperatureC64") == text(source_row, "rawTemperatureC64")
        && optional_text(&native.row, "rawRelativeHumidityPercent64")?
            == optional_text(source_row, "rawRelativeHumidityPercent64")?
        && optional_text(&native.row, "rawWindSpeedMps64")?
            == optional_text(source_row, "rawWindSpeedMps64")?
        && text(actual_key, "sourceId") == text(source_row, "bestMatchSourceId")
        && text(actual_key, "productRunAt") == text(source_row, "bestMatchProductRunAt")
        && text(&actual.row, "contentSha256") == text(source_row, "bestMatchContentSha256")
        && text(&actual.row, "validAt") == text(source_row, "validAt")
        && text(&native.row, "contentSha256") == text(source_row, "contentSha256")
        && text(&native.row, "validAt") == text(source_row, "validAt"))
}

/// Validate one exact retained physical source lineage and active interval.
fn validate_lineage(
    source: &JsonValue,
    lineage: &Lineage,
    valid_at: &str,
    wire: &NativeWirePrimitives,
) -> Result<(), NativeMaintenanceDailyTerminalValuesError> {
    use NativeMaintenanceDailyTerminalValuesError::Target;

    let valid = wire
        .parse_explicit_utc_milliseconds(valid_at)
        .or_else(|| wire.canonical_utc_milliseconds(valid_at))
        .ok_or(Target)?;
    // bind adapter and checked configuration before consulting station geometry
    if text(source, "adapterVersion").as_deref() != Some(lineage.adapter)
        || text(source, "sourceConfigFingerprint").as_deref() != Some(lineage.fingerprint)
        || text(source, "sourceKind").as_deref() != Some("physical_sensor")
        || lineage.start.is_some_and(|start| {
            wire.parse_explicit_utc_milliseconds(start)
                .is_none_or(|start| valid < start)
        })
        || lineage.end.is_some_and(|end| {
            wire.parse_explicit_utc_milliseconds(end)
                .is_none_or(|end| valid >= end)
        })
    {
        return Err(Target);
    }
    Ok(())
}

/// Validate the burned access receipt against the exact blinded plan identities.
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
    // cross-bind access state, plan roots, cutoff, and terminal clock
    if text(access, "accessState").as_deref() != Some("burned")
        || text(access, "registrationSha256") != text(registration, "registrationSha256")
        || text(access, "family") != text(registration, "family")
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

/// Build the exact complete or unsupported terminal graph identity.
fn terminal_graph(
    contract_version: &str,
    bindings: Vec<JsonValue>,
    derived_targets: &[NativeMaintenanceDailyDerivedTarget],
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
                derived_targets
                    .iter()
                    .map(|target| string(&target.target_member_sha256))
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

/// Validate one published occurrence envelope before any semantic decoding.
fn validate_occurrence_envelope(
    occurrence: &NativeMaintenanceHistoricalOccurrence,
    wire: &NativeWirePrimitives,
) -> Result<(), NativeMaintenanceDailyTerminalValuesError> {
    // bind all immutable payload and graph addresses from the verified archive
    if !valid_hash(&occurrence.graph_manifest_sha256)
        || !valid_hash(&occurrence.page_sha256)
        || !valid_hash(&occurrence.payload_identity_sha256)
        || format_digest(wire.sha256(&occurrence.payload_bytes))
            != occurrence.payload_identity_sha256
    {
        return Err(NativeMaintenanceDailyTerminalValuesError::History);
    }
    Ok(())
}

/// Validate document-level optional clocks against the immutable snapshot.
fn validate_document_clocks(
    document: &JsonValue,
    epoch: i64,
    cutoff: i64,
    wire: &NativeWirePrimitives,
) -> Result<(), NativeMaintenanceDailyTerminalValuesError> {
    use NativeMaintenanceDailyTerminalValuesError::Projection;

    let mut clocks = Vec::new();
    if let Some(clock) = optional_or_missing_text(document, "logicalReceivedAt")? {
        clocks.push(clock);
    }
    if let Some(key) = field(document, "logicalKey") {
        for name in ["productRunAt", "runInitializedAt"] {
            if let Some(clock) = optional_or_missing_text(key, name)? {
                clocks.push(clock);
            }
        }
    }
    // require every present document clock inside the epoch-to-cutoff snapshot
    if clocks
        .iter()
        .any(|clock| outside_snapshot(clock, epoch, cutoff, wire))
    {
        return Err(Projection);
    }
    Ok(())
}

/// Insert one unique content identity without arbitrary first-wins behavior.
fn insert_unique(
    index: &mut HashMap<String, IndexedProjection>,
    key: String,
    member: IndexedProjection,
) -> Result<(), NativeMaintenanceDailyTerminalValuesError> {
    // reject one content key resolving to distinct immutable archive authority
    if let Some(previous) = index.get(&key)
        && (previous.member_sha256 != member.member_sha256
            || previous.payload_identity_sha256 != member.payload_identity_sha256)
    {
        return Err(NativeMaintenanceDailyTerminalValuesError::Projection);
    }
    index.insert(key, member);
    Ok(())
}

/// Address one temperature comparator by its complete frozen tuple.
fn temperature_comparator_identity(
    member: &IndexedProjection,
    wire: &NativeWirePrimitives,
) -> Result<String, NativeMaintenanceDailyTerminalValuesError> {
    let key = field(&member.document, "logicalKey")
        .ok_or(NativeMaintenanceDailyTerminalValuesError::Projection)?;
    hash_value(
        wire,
        &object(vec![
            (
                "contentSha256",
                string(
                    &text(&member.row, "contentSha256")
                        .ok_or(NativeMaintenanceDailyTerminalValuesError::Projection)?,
                ),
            ),
            (
                "productRunAt",
                string(
                    &text(key, "productRunAt")
                        .ok_or(NativeMaintenanceDailyTerminalValuesError::Projection)?,
                ),
            ),
            (
                "sourceId",
                string(
                    &text(key, "sourceId")
                        .ok_or(NativeMaintenanceDailyTerminalValuesError::Projection)?,
                ),
            ),
            (
                "validAt",
                string(
                    &text(&member.row, "validAt")
                        .ok_or(NativeMaintenanceDailyTerminalValuesError::Projection)?,
                ),
            ),
        ]),
    )
}

/// Address one native temperature row by provider response, run, clock, and content.
fn temperature_native_identity(
    member: &IndexedProjection,
    wire: &NativeWirePrimitives,
) -> Result<String, NativeMaintenanceDailyTerminalValuesError> {
    let key = field(&member.document, "logicalKey")
        .ok_or(NativeMaintenanceDailyTerminalValuesError::Projection)?;
    hash_value(
        wire,
        &object(vec![
            (
                "contentSha256",
                string(
                    &text(&member.row, "contentSha256")
                        .ok_or(NativeMaintenanceDailyTerminalValuesError::Projection)?,
                ),
            ),
            (
                "providerResponseSha256",
                string(
                    &text(key, "providerResponseSha256")
                        .ok_or(NativeMaintenanceDailyTerminalValuesError::Projection)?,
                ),
            ),
            (
                "runInitializedAt",
                string(
                    &text(key, "runInitializedAt")
                        .ok_or(NativeMaintenanceDailyTerminalValuesError::Projection)?,
                ),
            ),
            (
                "validAt",
                string(
                    &text(&member.row, "validAt")
                        .ok_or(NativeMaintenanceDailyTerminalValuesError::Projection)?,
                ),
            ),
        ]),
    )
}

/// Project one exact graph member address.
fn graph_member(member: &IndexedProjection) -> JsonValue {
    object(vec![
        ("memberSha256", string(&member.member_sha256)),
        (
            "payloadIdentitySha256",
            string(&member.payload_identity_sha256),
        ),
        ("receiptSha256", string(&member.receipt.receiptSha256)),
    ])
}

/// Parse one exact preregistered capture key into due, zero-based row, and metric.
fn parse_expected_key(
    value: &str,
    family: NativeMaintenanceCaptureFamily,
) -> Option<(String, usize, Option<String>)> {
    let (prefix, metric) = value.rsplit_once('/')?;
    let (due_key, lead) = prefix.rsplit_once('/')?;
    let lead: usize = lead.parse().ok()?;
    if lead == 0 {
        return None;
    }
    let metric = match family {
        NativeMaintenanceCaptureFamily::Temperature if metric == "temperature" && lead <= 12 => {
            None
        }
        NativeMaintenanceCaptureFamily::Wind
            if matches!(metric, "windSpeedMps" | "windGustMps") && lead <= 168 =>
        {
            Some(metric.to_owned())
        }
        _ => return None,
    };
    Some((due_key.to_owned(), lead - 1, metric))
}

/// Return the exact retryable history-unavailable value.
fn pending_value(family: NativeMaintenanceCaptureFamily, registration_sha256: &str) -> JsonValue {
    object(vec![
        ("family", string(family.wire())),
        ("reason", string("history_unavailable")),
        ("registrationSha256", string(registration_sha256)),
        ("state", string("pending")),
    ])
}

/// Resolve one closed family spelling.
fn parse_family(value: &str) -> Option<NativeMaintenanceCaptureFamily> {
    match value {
        "temperature" => Some(NativeMaintenanceCaptureFamily::Temperature),
        "wind" => Some(NativeMaintenanceCaptureFamily::Wind),
        "rain" => Some(NativeMaintenanceCaptureFamily::Rain),
        _ => None,
    }
}

/// Return the exact generated projection-kind spelling.
fn receipt_kind(value: AdjustmentRevisionCommitReceiptProjectionKindEnum) -> &'static str {
    match value {
        AdjustmentRevisionCommitReceiptProjectionKindEnum::ActualBestMatch => "actual_best_match",
        AdjustmentRevisionCommitReceiptProjectionKindEnum::NativeSource => "native_source",
        AdjustmentRevisionCommitReceiptProjectionKindEnum::RainGateInput => "rain_gate_input",
        AdjustmentRevisionCommitReceiptProjectionKindEnum::ShadowPrediction => "shadow_prediction",
        AdjustmentRevisionCommitReceiptProjectionKindEnum::TargetRevision => "target_revision",
    }
}

/// Read one exact array item without cloning the surrounding document.
fn array_item<'a>(value: &'a JsonValue, name: &str, index: usize) -> Option<&'a JsonValue> {
    let JsonValue::Array(values) = field(value, name)? else {
        return None;
    };
    values.get(index)
}

/// Decode one nullable text field while preserving missing as an error.
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

/// Decode one optional text field where absent and explicit null share source semantics.
fn optional_or_missing_text(
    value: &JsonValue,
    name: &str,
) -> Result<Option<String>, NativeMaintenanceDailyTerminalValuesError> {
    match field(value, name) {
        None | Some(JsonValue::Null) => Ok(None),
        Some(JsonValue::String(units)) => String::from_utf16(units)
            .map(Some)
            .map_err(|_| NativeMaintenanceDailyTerminalValuesError::Projection),
        _ => Err(NativeMaintenanceDailyTerminalValuesError::Projection),
    }
}

/// Read one safe unsigned JSON integer.
fn integer(value: &JsonValue, name: &str) -> Option<usize> {
    match field(value, name)? {
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

/// Compare one canonical clock to the inclusive epoch-to-cutoff snapshot.
fn outside_snapshot(value: &str, epoch: i64, cutoff: i64, wire: &NativeWirePrimitives) -> bool {
    wire.canonical_utc_milliseconds(value)
        .is_none_or(|value| value < epoch || value > cutoff)
}

/// Decode exactly sixteen lowercase hexadecimal digits into one finite binary64.
fn decode_binary64(value: &str) -> Option<f64> {
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

/// Return the exact generated metric spelling.
fn metric_name(metric: ForecastAdjustmentMetric) -> &'static str {
    ForecastAdjustmentMetric::WIRE_VALUES
        .iter()
        .find_map(|(name, candidate)| (*candidate == metric).then_some(*name))
        .expect("generated metric is closed")
}

/// Return the exact generated physical station spelling.
fn station_name(station: ForecastObservationStationKey) -> Option<&'static str> {
    ForecastObservationStationKey::WIRE_VALUES
        .iter()
        .find_map(|(name, candidate)| (*candidate == station).then_some(*name))
}

/// Return one exact retained source lineage by its active source key.
fn source_lineage(source_key: &str) -> Option<&'static Lineage> {
    SOURCE_LINEAGES
        .iter()
        .find(|lineage| lineage.source_key == source_key)
}

const SOURCE_LINEAGES: &[Lineage] = &[
    Lineage {
        source_key: "ambient-maxweather-observations-v1",
        adapter: "ambient-device-data/v1",
        fingerprint: "7a7528a6278924ca5280a1a6045b6647b7e660b112d7fa3008c542a17ff99df4",
        station: ForecastObservationStationKey::AmbientMaxweather,
        start: Some("2026-08-24T00:00:00Z"),
        end: None,
    },
    Lineage {
        source_key: "ambient-merlin-observations-v1",
        adapter: "ambient-device-data/v1",
        fingerprint: "c3829701bfc25a050022dc3965569d3a87376e8a43b5fdcb7621533f1ae3c65d",
        station: ForecastObservationStationKey::AmbientMerlin,
        start: Some("2021-01-01T00:00:00Z"),
        end: None,
    },
    Lineage {
        source_key: "ecowitt-88f15505d89f-local-live-v1",
        adapter: "ecowitt-local-live/v1",
        fingerprint: "0a44488714d0fa807b924f8aea14965b437722e8cf9f8eae4bc8c81da8a0149d",
        station: ForecastObservationStationKey::BallydideanEcowitt,
        start: None,
        end: None,
    },
    Lineage {
        source_key: "netatmo-nearby-observations-v1",
        adapter: "netatmo-public-measures/v1",
        fingerprint: "5495917dd2465a32d9878e73c68781a229b432901cdc4867875726351efbdbbc",
        station: ForecastObservationStationKey::NetatmoNearby,
        start: Some("2022-06-21T00:00:00Z"),
        end: None,
    },
    Lineage {
        source_key: "tempest-126537-observations-v2",
        adapter: "tempest-observations/v2",
        fingerprint: "34dafbd6584c93d55ed4d3d43dc7e74a0876165d4ddfc921413f7b826dff7ab7",
        station: ForecastObservationStationKey::Tempest126537,
        start: Some("2023-12-17T00:00:00Z"),
        end: None,
    },
    Lineage {
        source_key: "tempest-168853-observations-v2",
        adapter: "tempest-observations/v2",
        fingerprint: "1c7a402337a44a5441775246cbc02da7994599dad6ab83dc04b248303facfea5",
        station: ForecastObservationStationKey::Tempest168853,
        start: Some("2025-01-22T00:00:00Z"),
        end: None,
    },
    Lineage {
        source_key: "tempest-201058-observations-v2",
        adapter: "tempest-observations/v2",
        fingerprint: "a61cce798cddf682da9608dc245659fc7734a6d5304068939d3115ae7d81a50e",
        station: ForecastObservationStationKey::Tempest201058,
        start: Some("2025-12-22T00:00:00Z"),
        end: None,
    },
    Lineage {
        source_key: "tempest-203055-observations-v2",
        adapter: "tempest-observations/v2",
        fingerprint: "9ead4c5359a6a9640f334be91397180aa62b90b0f0ce813b9ff26fe84537acc4",
        station: ForecastObservationStationKey::Tempest203055,
        start: Some("2025-12-25T00:00:00Z"),
        end: None,
    },
    Lineage {
        source_key: "tempest-225947-observations-v2",
        adapter: "tempest-observations/v2",
        fingerprint: "b4dd6105d9a56a7c5d0dc4063f830e1cf28d693222a8de15536dd83d3a6178c4",
        station: ForecastObservationStationKey::Tempest225947,
        start: Some("2026-07-14T00:00:00Z"),
        end: None,
    },
    Lineage {
        source_key: "tempest-38270-observations-v2",
        adapter: "tempest-observations/v2",
        fingerprint: "ce162067aced4ab3522fb83145a21e608ff24dec189097726188e96fd6cca52f",
        station: ForecastObservationStationKey::Tempest38270,
        start: Some("2021-01-04T00:00:00Z"),
        end: None,
    },
    Lineage {
        source_key: "tempest-64255-observations-v2",
        adapter: "tempest-observations/v2",
        fingerprint: "8eb488a358375fc3526347d9ef6c9f23080095a22ea874a42ec400b0317d868a",
        station: ForecastObservationStationKey::Tempest64255,
        start: Some("2021-12-10T00:00:00Z"),
        end: None,
    },
    Lineage {
        source_key: "wunderground-maxweather-history-v1",
        adapter: "wunderground-pws-history/v1",
        fingerprint: "52dda6c5444d0a234fbe23d6218027d417ac966ecf291a7d5dfff42fd0dc207c",
        station: ForecastObservationStationKey::AmbientMaxweather,
        start: Some("2024-11-29T00:00:00Z"),
        end: Some("2026-08-24T00:00:00Z"),
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// Compare one all-missing physical terminal part with the actual predecessor builder.
    #[test]
    fn native_terminal_unsupported_part_matches_source() {
        let wire = NativeWirePrimitives::new();
        let parts = expected_parts(
            NativeMaintenanceCaptureFamily::Temperature,
            "2026-11-01T07:00:00.000Z",
            "2027-11-02T07:00:00.000Z",
            &wire,
        )
        .expect("enumerate native expected parts");
        let part = parts.first().expect("first native expected part");
        let chunk = build_chunk(
            part,
            NativeMaintenanceCaptureFamily::Temperature,
            &"a".repeat(64),
            &"b".repeat(64),
            Vec::new(),
            part.expected_keys.clone(),
            &wire,
        )
        .expect("build native unsupported part");
        let native = object(vec![
            ("metadata", chunk.metadata),
            ("payloadBase64", string(&chunk.payload_base64)),
        ]);
        let native = canonical_bytes(&wire, &native).expect("encode native unsupported part");
        let repository = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let output = Command::new("node")
            .args(["--input-type=module", "--eval", NODE_ORACLE])
            .current_dir(repository)
            .output()
            .expect("run predecessor unsupported part builder");
        assert!(
            output.status.success(),
            "source unsupported part failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(native, output.stdout);
    }

    const NODE_ORACLE: &str = r#"
import {
  buildAdjustmentMaintenanceEvaluationChunkV3,
  buildAdjustmentMaintenanceExpectedKeyPlanV3,
} from "./scripts/research/adjustment_daily_evaluation.mjs";
import {
  canonicalJsonBytes,
} from "./scripts/research/adjustment-maintenance-runtime/forecast/candidate.js";

const plan = buildAdjustmentMaintenanceExpectedKeyPlanV3({
  family: "temperature",
  intervalEndAt: "2027-11-02T07:00:00.000Z",
  intervalStartAt: "2026-11-01T07:00:00.000Z",
});
const logical = plan.logicalChunks[0];
const part = logical.parts[0];
const chunk = buildAdjustmentMaintenanceEvaluationChunkV3({
  captureLocalDate: part.captureLocalDate,
  confirmationRegistrationSha256: "a".repeat(64),
  family: "temperature",
  fromLocalDate: logical.fromLocalDate,
  logicalChunkIndex: logical.logicalChunkIndex,
  missingKeys: [...part.expectedKeys],
  partCount: part.partCount,
  partIndex: part.partIndex,
  records: [],
  shadowRegistrationSha256: "b".repeat(64),
  toLocalDateExclusive: logical.toLocalDateExclusive,
});
process.stdout.write(canonicalJsonBytes(chunk));
"#;
}
