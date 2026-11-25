//! Value-blind daily terminal planning over a verified historical archive.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Display, Formatter};

use weather_contracts::{FieldPresence, JsonValue};
use weather_edge::{
    ArchiveWirePrimitives, PrivateShadowArchiveVersion, PrivateShadowReceiptCodec,
    RevisionArchiveCanonicalCodec,
};

use crate::maintenance_canonical_json::{exact_keys, field, text, valid_hash};
use crate::native_maintenance_policy_comparison::{object, string};
use crate::native_maintenance_retirement_files_v2::format_digest;
use crate::{
    NativeAdjustmentRollingShadowRegistration, NativeMaintenanceCaptureFamily,
    NativeMaintenanceDue, NativeMaintenanceDueMode, NativeMaintenanceHistoricalArchiveIndex,
    NativeModelEvidenceValues, NativeShadowProducerCodec, NativeWirePrimitives,
    VerifiedNativeMaintenanceCaptureEpochWitness, build_native_maintenance_expected_key_plan,
};

const HOUR_MILLISECONDS: i64 = 3_600_000;
const CYCLE_MILLISECONDS: i64 = 6 * HOUR_MILLISECONDS;
const GENESIS_FRONTIER_SHA256: &str =
    "5d932b9623819be9432877a513194158708719497d755155f5ed9a4b501b48af";
const FINAL_V14_MIGRATION_HISTORY_SHA256: &str =
    "6de5c8c7efaa448aeb12bf1a9debe6fe7d4d4d1003ee0e21ab619ffa624c3424";
const WITNESS_KEYS: &[&str] = &[
    "activationKind",
    "archiveCommitOrdinal",
    "catalogFrontierSha256",
    "contractVersion",
    "controlPlaneSha256",
    "controlPlaneVersion",
    "databaseMigrationHistorySha256",
    "epochAt",
    "servingSnapshotSha256",
    "sourceCommit",
    "sourceRelease",
    "sourceServerImageDigest",
    "sourceWebImageDigest",
    "witnessSha256",
];
const SHADOW_KEYS: &[&str] = &[
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

/// Exact source inputs after lifecycle and archive ownership have been independently verified.
pub struct NativeMaintenanceDailyTerminalPreparationInput<'a> {
    pub catalog_input_manifest_sha256: &'a str,
    pub clock_at: &'a str,
    pub confirmation_registration: Option<&'a JsonValue>,
    pub due: &'a NativeMaintenanceDue,
    pub epoch_witness: &'a JsonValue,
    pub history: &'a NativeMaintenanceHistoricalArchiveIndex,
    pub shadow_registration: &'a NativeAdjustmentRollingShadowRegistration,
    pub source_commit: &'a str,
    pub verified_epoch_witness: &'a VerifiedNativeMaintenanceCaptureEpochWitness,
}

/// Closed value-blind preparation state before any irreversible terminal effects.
#[derive(Clone, Debug, PartialEq)]
pub enum NativeMaintenanceDailyTerminalPreparation {
    Pending { daily_evaluation_input: JsonValue },
    Supported { plan: JsonValue },
    Unsupported { plan: JsonValue },
}

impl NativeMaintenanceDailyTerminalPreparation {
    /// Return the exact source state represented by this effect-free preparation.
    pub const fn state(&self) -> &'static str {
        // preserve the source terminal branch names without claiming downstream effects
        match self {
            Self::Pending { .. } => "pending",
            Self::Supported { .. } => "planned",
            Self::Unsupported { .. } => "unsupported_planned",
        }
    }

    /// Return the source plan only after the terminal clock has opened.
    pub const fn plan(&self) -> Option<&JsonValue> {
        // pending preparation has no archive plan to publish or burn
        match self {
            Self::Pending { .. } => None,
            Self::Supported { plan } | Self::Unsupported { plan } => Some(plan),
        }
    }
}

/// Closed input, archive, or codec rejection before terminal effects are allowed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceDailyTerminalPreparationError {
    Input,
    Witness,
    Registration,
    History,
    Capsule,
    Codec,
}

impl Display for NativeMaintenanceDailyTerminalPreparationError {
    /// Render one bounded diagnostic without reflecting retained archive material.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        // keep externally observable failures bounded by their owning admission stage
        formatter.write_str(match self {
            Self::Input => "daily terminal preparation input is invalid",
            Self::Witness => "daily terminal epoch witness differs",
            Self::Registration => "daily terminal registration differs",
            Self::History => "daily terminal historical archive differs",
            Self::Capsule => "daily terminal confirmation capsule differs",
            Self::Codec => "daily terminal preparation serialization failed",
        })
    }
}

impl std::error::Error for NativeMaintenanceDailyTerminalPreparationError {}

#[derive(Clone)]
struct ConfirmationRegistration {
    candidate_sha256: String,
    family: NativeMaintenanceCaptureFamily,
    registration_sha256: String,
    reserved_key_sha256: String,
    source_lineage_sha256: String,
    terminal_access_at: String,
}

#[derive(Clone)]
struct ShadowRegistration {
    candidate_sha256: String,
    family: NativeMaintenanceCaptureFamily,
    interval_end_at: String,
    interval_start_at: String,
    registration_sha256: String,
    reserved_key_sha256: String,
    source_sha256: String,
    target_cutoff_at: String,
    terminal_at: String,
}

#[derive(Clone)]
struct ConfirmationEntry {
    comparator_member_sha256: String,
    due_key: String,
    graph_manifest_sha256: String,
    payload_identity_sha256: String,
    prediction_body_sha256: String,
    receipt_sha256: String,
    source_projection_sha256: String,
    source_receipt_sha256: String,
}

struct ExpectedPopulation {
    due_keys: BTreeSet<String>,
    entry_count: usize,
    reserved_key_sha256: String,
}

/// Prepare the exact pending evaluation input or terminal value-blind confirmation plan.
pub fn prepare_native_maintenance_daily_terminal_material(
    input: NativeMaintenanceDailyTerminalPreparationInput<'_>,
) -> Result<NativeMaintenanceDailyTerminalPreparation, NativeMaintenanceDailyTerminalPreparationError>
{
    use NativeMaintenanceDailyTerminalPreparationError as Error;

    let wire = NativeWirePrimitives::new();
    let clock = wire
        .canonical_utc_milliseconds(input.clock_at)
        .ok_or(Error::Input)?;
    // close caller identities before selecting any retained archive material
    if !valid_hash(input.catalog_input_manifest_sha256)
        || !valid_git_commit(input.source_commit)
        || !valid_daily_due(input.due, &wire)
    {
        return Err(Error::Input);
    }
    validate_epoch_witness(input.epoch_witness, input.verified_epoch_witness, &wire)?;
    let shadow = validate_shadow_registration(
        input.shadow_registration,
        &input.verified_epoch_witness.witness_sha256,
        &wire,
    )?;
    let terminal = wire
        .canonical_utc_milliseconds(&shadow.terminal_at)
        .ok_or(Error::Registration)?;

    // retain the unopened evaluation surface until the database-owned terminal clock
    if clock < terminal {
        let registration_sha256 = input
            .confirmation_registration
            .and_then(|value| text(value, "registrationSha256"))
            .filter(|value| valid_hash(value));
        let lifecycle = object(vec![
            ("family", string(shadow.family.wire())),
            (
                "registrationSha256",
                registration_sha256
                    .as_deref()
                    .map(string)
                    .unwrap_or(JsonValue::Null),
            ),
            ("state", string("pending")),
        ]);
        let daily_evaluation_input = object(vec![
            (
                "catalogInputManifestSha256",
                string(input.catalog_input_manifest_sha256),
            ),
            ("clockAt", string(input.clock_at)),
            ("due", due_value(input.due)),
            ("epochWitness", input.epoch_witness.clone()),
            ("lifecycle", lifecycle),
        ]);
        return Ok(NativeMaintenanceDailyTerminalPreparation::Pending {
            daily_evaluation_input,
        });
    }

    let registration = validate_confirmation_registration(
        input.confirmation_registration.ok_or(Error::Registration)?,
        &wire,
    )?;
    // require the local preregistration to remain cross-bound to the active public release
    if registration.family != shadow.family
        || registration.candidate_sha256 != shadow.candidate_sha256
        || registration.source_lineage_sha256 != shadow.source_sha256
        || registration.reserved_key_sha256 != shadow.reserved_key_sha256
        || registration.terminal_access_at != shadow.terminal_at
    {
        return Err(Error::Registration);
    }
    validate_history(input.history)?;
    let expected = expected_population(&shadow, &wire)?;
    // require both preregistration domains to bind the complete frozen population
    if expected.reserved_key_sha256 != shadow.reserved_key_sha256
        || expected.reserved_key_sha256 != registration.reserved_key_sha256
    {
        return Err(Error::Registration);
    }
    let entries = collect_entries(
        input.history,
        &shadow,
        input.verified_epoch_witness,
        &expected.due_keys,
        &wire,
    )?;
    // choose the complete plan only when every preregistered capture cycle is present
    if entries.len() == expected.due_keys.len()
        && expected
            .due_keys
            .iter()
            .all(|due_key| entries.contains_key(due_key))
    {
        let plan = build_supported_plan(
            input.history,
            &registration,
            &shadow,
            &expected,
            entries.into_values().collect(),
            &wire,
        )?;
        return Ok(NativeMaintenanceDailyTerminalPreparation::Supported { plan });
    }
    let plan = build_unsupported_plan(
        input.history,
        &registration,
        &shadow,
        &expected,
        entries,
        &wire,
    )?;
    Ok(NativeMaintenanceDailyTerminalPreparation::Unsupported { plan })
}

/// Validate and cross-bind the complete canonical witness retained by the caller authority.
fn validate_epoch_witness(
    value: &JsonValue,
    verified: &VerifiedNativeMaintenanceCaptureEpochWitness,
    wire: &NativeWirePrimitives,
) -> Result<(), NativeMaintenanceDailyTerminalPreparationError> {
    use NativeMaintenanceDailyTerminalPreparationError::Witness;

    // require the exact original closed witness before comparing the verified projection
    if !exact_keys(value, WITNESS_KEYS)
        || text(value, "activationKind").as_deref() != Some("inert_v14_pre_activation")
        || text(value, "archiveCommitOrdinal").as_deref() != Some("0")
        || text(value, "contractVersion").as_deref()
            != Some("adjustment-revision-capture-epoch-witness/v1")
    {
        return Err(Witness);
    }
    let bindings = [
        ("catalogFrontierSha256", &verified.catalog_frontier_sha256),
        ("controlPlaneSha256", &verified.control_plane_sha256),
        ("controlPlaneVersion", &verified.control_plane_version),
        (
            "databaseMigrationHistorySha256",
            &verified.database_migration_history_sha256,
        ),
        ("epochAt", &verified.epoch_at),
        ("servingSnapshotSha256", &verified.serving_snapshot_sha256),
        ("sourceCommit", &verified.source_commit),
        ("sourceRelease", &verified.source_release),
        (
            "sourceServerImageDigest",
            &verified.source_server_image_digest,
        ),
        ("witnessSha256", &verified.witness_sha256),
    ];
    // bind every field retained by the independently verified typed projection
    if bindings
        .iter()
        .any(|(name, expected)| text(value, name).as_deref() != Some(expected.as_str()))
        || verified.catalog_frontier_sha256 != GENESIS_FRONTIER_SHA256
        || verified.control_plane_version != "14"
        || verified.database_migration_history_sha256 != FINAL_V14_MIGRATION_HISTORY_SHA256
        || !valid_hash(&verified.catalog_frontier_sha256)
        || !valid_hash(&verified.control_plane_sha256)
        || !valid_hash(&verified.database_migration_history_sha256)
        || !valid_hash(&verified.serving_snapshot_sha256)
        || !valid_hash(&verified.witness_sha256)
        || wire
            .canonical_utc_milliseconds(&verified.epoch_at)
            .is_none()
        || !valid_git_commit(&verified.source_commit)
        || !valid_release(&verified.source_release)
    {
        return Err(Witness);
    }
    let source_web_image_digest = text(value, "sourceWebImageDigest").ok_or(Witness)?;
    // retain both immutable image digest grammars even though readiness omits the web value
    if !valid_image_digest(&verified.source_server_image_digest)
        || !valid_image_digest(&source_web_image_digest)
    {
        return Err(Witness);
    }
    let unsigned = without_field(value, "witnessSha256").ok_or(Witness)?;
    let encoded = wire.encode_sorted_json(&unsigned).ok_or(Witness)?;
    // recompute the full witness identity so the omitted web digest remains authenticated
    if format_digest(wire.sha256(&encoded)) != verified.witness_sha256 {
        return Err(Witness);
    }
    Ok(())
}

/// Decode and authenticate the exact active public rolling registration.
fn validate_shadow_registration(
    source: &NativeAdjustmentRollingShadowRegistration,
    epoch_witness_sha256: &str,
    wire: &NativeWirePrimitives,
) -> Result<ShadowRegistration, NativeMaintenanceDailyTerminalPreparationError> {
    use NativeMaintenanceDailyTerminalPreparationError::Registration;

    let value = &source.registration;
    // admit only the closed source registration grammar and fixed site ownership
    if !exact_keys(value, SHADOW_KEYS)
        || text(value, "siteKey").as_deref() != Some("ballydidean")
        || text(value, "epochWitnessSha256").as_deref() != Some(epoch_witness_sha256)
    {
        return Err(Registration);
    }
    let family = family(&text(value, "family").ok_or(Registration)?).ok_or(Registration)?;
    let hash_fields = [
        "artifactSha256",
        "candidateSha256",
        "cohortSha256",
        "epochWitnessSha256",
        "policySha256",
        "registrationSha256",
        "reservedKeySha256",
        "scheduleContractSha256",
        "sourceSha256",
    ];
    // close every independently supplied identity before recreating the preimage
    if hash_fields
        .iter()
        .any(|name| text(value, name).is_none_or(|identity| !valid_hash(&identity)))
    {
        return Err(Registration);
    }
    let predecessor = field(value, "predecessorRegistrationSha256").ok_or(Registration)?;
    let predecessor = match predecessor {
        JsonValue::Null => None,
        JsonValue::String(_) => {
            Some(text(value, "predecessorRegistrationSha256").ok_or(Registration)?)
        }
        _ => return Err(Registration),
    };
    // admit only null or one canonical predecessor identity
    if predecessor
        .as_deref()
        .is_some_and(|identity| !valid_hash(identity))
    {
        return Err(Registration);
    }
    let interval_start_at = text(value, "intervalStartAt").ok_or(Registration)?;
    let interval_end_at = text(value, "intervalEndAt").ok_or(Registration)?;
    let target_cutoff_at = text(value, "targetCutoffAt").ok_or(Registration)?;
    let terminal_at = text(value, "terminalAt").ok_or(Registration)?;
    // require the four public clocks in exact canonical UTC millisecond form
    if [
        interval_start_at.as_str(),
        interval_end_at.as_str(),
        target_cutoff_at.as_str(),
        terminal_at.as_str(),
    ]
    .into_iter()
    .any(|instant| wire.canonical_utc_milliseconds(instant).is_none())
    {
        return Err(Registration);
    }
    let candidate_sha256 = text(value, "candidateSha256").ok_or(Registration)?;
    let reserved_key_sha256 = text(value, "reservedKeySha256").ok_or(Registration)?;
    let source_sha256 = text(value, "sourceSha256").ok_or(Registration)?;
    let registration_sha256 = text(value, "registrationSha256").ok_or(Registration)?;
    let preimage = format!(
        "adjustment-shadow-registration/v3\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n",
        text(value, "siteKey").ok_or(Registration)?,
        family.wire(),
        candidate_sha256,
        text(value, "artifactSha256").ok_or(Registration)?,
        text(value, "policySha256").ok_or(Registration)?,
        text(value, "cohortSha256").ok_or(Registration)?,
        reserved_key_sha256,
        source_sha256,
        epoch_witness_sha256,
        text(value, "scheduleContractSha256").ok_or(Registration)?,
        predecessor.as_deref().unwrap_or("none"),
        interval_start_at,
        interval_end_at,
        target_cutoff_at,
        terminal_at,
    );
    // bind the public object to its noncircular newline-framed registration identity
    if format_digest(wire.sha256(preimage.as_bytes())) != registration_sha256
        || source.registration_sha256 != registration_sha256
    {
        return Err(Registration);
    }
    Ok(ShadowRegistration {
        candidate_sha256,
        family,
        interval_end_at,
        interval_start_at,
        registration_sha256,
        reserved_key_sha256,
        source_sha256,
        target_cutoff_at,
        terminal_at,
    })
}

/// Decode the local preregistration fields used by the source value-blind planner.
fn validate_confirmation_registration(
    value: &JsonValue,
    wire: &NativeWirePrimitives,
) -> Result<ConfirmationRegistration, NativeMaintenanceDailyTerminalPreparationError> {
    use NativeMaintenanceDailyTerminalPreparationError::Registration;

    let family = family(&text(value, "family").ok_or(Registration)?).ok_or(Registration)?;
    let candidate_sha256 = text(value, "candidateSha256").ok_or(Registration)?;
    let registration_sha256 = text(value, "registrationSha256").ok_or(Registration)?;
    let reserved_key_sha256 = text(value, "reservedKeySha256").ok_or(Registration)?;
    let source_lineage_sha256 = text(value, "sourceLineageSha256").ok_or(Registration)?;
    let terminal_access_at = text(value, "terminalAccessAt").ok_or(Registration)?;
    // require the same public fields and clocks consumed by the predecessor planner
    if [
        candidate_sha256.as_str(),
        registration_sha256.as_str(),
        reserved_key_sha256.as_str(),
        source_lineage_sha256.as_str(),
    ]
    .into_iter()
    .any(|identity| !valid_hash(identity))
        || text(value, "intervalStartLocalDate").is_none()
        || text(value, "intervalEndExclusiveLocalDate").is_none()
        || wire
            .canonical_utc_milliseconds(&terminal_access_at)
            .is_none()
    {
        return Err(Registration);
    }
    Ok(ConfirmationRegistration {
        candidate_sha256,
        family,
        registration_sha256,
        reserved_key_sha256,
        source_lineage_sha256,
        terminal_access_at,
    })
}

/// Validate the complete-frontier identity required by confirmation planning.
fn validate_history(
    history: &NativeMaintenanceHistoricalArchiveIndex,
) -> Result<(), NativeMaintenanceDailyTerminalPreparationError> {
    use NativeMaintenanceDailyTerminalPreparationError::History;

    // preserve the source nonempty history and canonical complete-frontier watermark contract
    if history.contract_version != "adjustment-revision-historical-archive-index/v1"
        || history.occurrences.is_empty()
        || !valid_hash(&history.history_root_sha256)
        || !valid_hash(&history.catalog.catalog_root_sha256)
        || !valid_hash(&history.catalog.frontier_root_sha256)
        || !valid_archive_ordinal(&history.catalog.archive_commit_ordinal)
    {
        return Err(History);
    }
    Ok(())
}

/// Recreate the complete due-cycle set and cell count behind the compact expected-key plan.
fn expected_population(
    shadow: &ShadowRegistration,
    wire: &NativeWirePrimitives,
) -> Result<ExpectedPopulation, NativeMaintenanceDailyTerminalPreparationError> {
    use NativeMaintenanceDailyTerminalPreparationError::Registration;

    let plan = build_native_maintenance_expected_key_plan(
        shadow.family,
        &shadow.interval_start_at,
        &shadow.interval_end_at,
    )
    .ok_or(Registration)?;
    let start = wire
        .canonical_utc_milliseconds(&shadow.interval_start_at)
        .ok_or(Registration)?;
    let end = wire
        .canonical_utc_milliseconds(&shadow.interval_end_at)
        .ok_or(Registration)?;
    let (first_lead, row_count) = match shadow.family {
        NativeMaintenanceCaptureFamily::Temperature => (1_i64, 12_i64),
        NativeMaintenanceCaptureFamily::Wind => (1, 168),
        NativeMaintenanceCaptureFamily::Rain => (9, 23),
    };
    let maximum_lead = first_lead + row_count - 1;
    let mut cycle = start
        .checked_sub(maximum_lead * HOUR_MILLISECONDS)
        .ok_or(Registration)?
        .div_euclid(CYCLE_MILLISECONDS)
        * CYCLE_MILLISECONDS;
    let mut due_keys = BTreeSet::new();
    let mut entry_count = 0_usize;
    // enumerate every source cycle whose target population intersects the registered window
    while cycle < end {
        let mut present = false;
        for row in 0..row_count {
            let model_lead = first_lead + row;
            let valid = cycle
                .checked_add(model_lead * HOUR_MILLISECONDS)
                .ok_or(Registration)?;
            // omit target rows outside the half-open rolling interval
            if valid < start || valid >= end {
                continue;
            }
            present = true;
            entry_count = entry_count
                .checked_add(match shadow.family {
                    NativeMaintenanceCaptureFamily::Wind if (49..=72).contains(&model_lead) => 1,
                    NativeMaintenanceCaptureFamily::Wind => 2,
                    NativeMaintenanceCaptureFamily::Temperature
                    | NativeMaintenanceCaptureFamily::Rain => 1,
                })
                .ok_or(Registration)?;
        }
        // retain one unique capture due key for every intersecting source cycle
        if present {
            let due_at = wire
                .format_utc_milliseconds(cycle.checked_add(35 * 60_000).ok_or(Registration)?)
                .ok_or(Registration)?;
            due_keys.insert(format!("capture/{due_at}"));
        }
        cycle = cycle.checked_add(CYCLE_MILLISECONDS).ok_or(Registration)?;
    }
    Ok(ExpectedPopulation {
        due_keys,
        entry_count,
        reserved_key_sha256: plan.reserved_key_sha256,
    })
}

/// Select authenticated value-blind v2 capsule identities in canonical due-key order.
fn collect_entries(
    history: &NativeMaintenanceHistoricalArchiveIndex,
    shadow: &ShadowRegistration,
    witness: &VerifiedNativeMaintenanceCaptureEpochWitness,
    expected_due_keys: &BTreeSet<String>,
    wire: &NativeWirePrimitives,
) -> Result<BTreeMap<String, ConfirmationEntry>, NativeMaintenanceDailyTerminalPreparationError> {
    use NativeMaintenanceDailyTerminalPreparationError::Capsule;

    let epoch = wire
        .canonical_utc_milliseconds(&witness.epoch_at)
        .ok_or(Capsule)?;
    let cutoff = wire
        .canonical_utc_milliseconds(&shadow.target_cutoff_at)
        .ok_or(Capsule)?;
    let codec = NativeShadowProducerCodec::new(NativeModelEvidenceValues::new());
    let mut entries = BTreeMap::new();
    // inspect only published v2 shadow capsules and leave unrelated archive members unopened
    for occurrence in &history.occurrences {
        if occurrence.publication_disposition != "published"
            || occurrence.payload_kind != "adjustment-shadow-revision-capsule/v2"
        {
            continue;
        }
        // authenticate the selected immutable envelope before semantic decoding
        if !valid_hash(&occurrence.graph_manifest_sha256)
            || !valid_hash(&occurrence.page_sha256)
            || !valid_hash(&occurrence.payload_identity_sha256)
            || format_digest(wire.sha256(&occurrence.payload_bytes))
                != occurrence.payload_identity_sha256
        {
            return Err(Capsule);
        }
        let capsule =
            PrivateShadowReceiptCodec::decode_capsule_document(&codec, &occurrence.payload_bytes)
                .ok_or(Capsule)?;
        // require the value-blind v2 comparator and prediction clock grammar
        if capsule.version != PrivateShadowArchiveVersion::V2 {
            return Err(Capsule);
        }
        let due_key = capsule.stage_input.metadata.dueKey.clone();
        // omit other registrations and cycles only after parser authentication
        if capsule.stage_input.metadata.registrationSha256 != shadow.registration_sha256
            || !expected_due_keys.contains(&due_key)
        {
            continue;
        }
        let prediction_committed_at = match &capsule.prediction_committed_at {
            FieldPresence::Value(value) => value,
            FieldPresence::Missing => return Err(Capsule),
        };
        let comparator_member_sha256 = match &capsule.stage_receipt.comparatorSha256 {
            FieldPresence::Value(value) => value.clone(),
            FieldPresence::Missing => return Err(Capsule),
        };
        let receipt = occurrence.receipts.first().ok_or(Capsule)?;
        let committed = wire
            .canonical_utc_milliseconds(&receipt.archiveCommittedAt)
            .ok_or(Capsule)?;
        let predicted = wire
            .canonical_utc_milliseconds(prediction_committed_at)
            .ok_or(Capsule)?;
        // bind the sole cold receipt and both cutoff clocks to the retained capsule
        if occurrence.receipts.len() != 1
            || receipt != &capsule.revision_receipt
            || committed < epoch
            || committed > cutoff
            || predicted > cutoff
            || entries.contains_key(&due_key)
        {
            return Err(Capsule);
        }
        entries.insert(
            due_key.clone(),
            ConfirmationEntry {
                comparator_member_sha256,
                due_key,
                graph_manifest_sha256: occurrence.graph_manifest_sha256.clone(),
                payload_identity_sha256: occurrence.payload_identity_sha256.clone(),
                prediction_body_sha256: capsule.stage_input.metadata.predictionBodySha256,
                receipt_sha256: receipt.receiptSha256.clone(),
                source_projection_sha256: capsule.stage_input.sourceProjectionSha256,
                source_receipt_sha256: capsule.stage_input.metadata.sourceReceiptSha256,
            },
        );
    }
    Ok(entries)
}

/// Build the exact complete value-blind source plan after all due cycles are present.
fn build_supported_plan(
    history: &NativeMaintenanceHistoricalArchiveIndex,
    registration: &ConfirmationRegistration,
    shadow: &ShadowRegistration,
    expected: &ExpectedPopulation,
    entries: Vec<ConfirmationEntry>,
    wire: &NativeWirePrimitives,
) -> Result<JsonValue, NativeMaintenanceDailyTerminalPreparationError> {
    use NativeMaintenanceDailyTerminalPreparationError::Codec;

    let entry_values = JsonValue::Array(entries.iter().map(entry_value).collect());
    let eligible_prediction_set_sha256 = hash_value(wire, &entry_values).ok_or(Codec)?;
    let plan_graph_unsigned = object(vec![
        (
            "contractVersion",
            string("adjustment-maintenance-confirmation-plan-graph/v1"),
        ),
        (
            "eligiblePredictionSetSha256",
            string(&eligible_prediction_set_sha256),
        ),
        ("entries", entry_values),
        ("family", string(registration.family.wire())),
        (
            "registrationSha256",
            string(&registration.registration_sha256),
        ),
    ]);
    let plan_graph_sha256 = hash_value(wire, &plan_graph_unsigned).ok_or(Codec)?;
    let plan_graph = with_field(
        &plan_graph_unsigned,
        "planGraphSha256",
        string(&plan_graph_sha256),
    )
    .ok_or(Codec)?;
    let revision_catalog_watermark_sha256 = history_watermark(history, wire).ok_or(Codec)?;
    let snapshot_root_sha256 = hash_value(
        wire,
        &object(vec![
            (
                "eligiblePredictionSetSha256",
                string(&eligible_prediction_set_sha256),
            ),
            (
                "expectedKeySetSha256",
                string(&expected.reserved_key_sha256),
            ),
            ("targetCutoffAt", string(&shadow.target_cutoff_at)),
        ]),
    )
    .ok_or(Codec)?;
    Ok(object(vec![
        (
            "contractVersion",
            string("adjustment-maintenance-confirmation-value-plan/v1"),
        ),
        (
            "eligiblePredictionSetSha256",
            string(&eligible_prediction_set_sha256),
        ),
        ("entryCount", JsonValue::Number(expected.entry_count as f64)),
        (
            "expectedKeySetSha256",
            string(&expected.reserved_key_sha256),
        ),
        ("family", string(registration.family.wire())),
        ("planGraph", plan_graph),
        (
            "registrationSha256",
            string(&registration.registration_sha256),
        ),
        (
            "revisionCatalogWatermarkSha256",
            string(&revision_catalog_watermark_sha256),
        ),
        ("snapshotRootSha256", string(&snapshot_root_sha256)),
        ("state", string("planned")),
        ("targetCutoffAt", string(&shadow.target_cutoff_at)),
    ]))
}

/// Build the exact disjoint unsupported plan with every permanently absent due cycle named.
fn build_unsupported_plan(
    history: &NativeMaintenanceHistoricalArchiveIndex,
    registration: &ConfirmationRegistration,
    shadow: &ShadowRegistration,
    expected: &ExpectedPopulation,
    entries: BTreeMap<String, ConfirmationEntry>,
    wire: &NativeWirePrimitives,
) -> Result<JsonValue, NativeMaintenanceDailyTerminalPreparationError> {
    use NativeMaintenanceDailyTerminalPreparationError::Codec;

    let missing_due_keys: Vec<String> = expected
        .due_keys
        .iter()
        .filter(|due_key| !entries.contains_key(*due_key))
        .cloned()
        .collect();
    let entry_values = JsonValue::Array(entries.values().map(entry_value).collect());
    let missing_values = JsonValue::Array(
        missing_due_keys
            .iter()
            .map(|due_key| string(due_key))
            .collect(),
    );
    let eligible_prediction_set_sha256 = hash_value(wire, &entry_values).ok_or(Codec)?;
    let missing_due_key_set_sha256 = hash_value(wire, &missing_values).ok_or(Codec)?;
    let plan_graph_unsigned = object(vec![
        (
            "contractVersion",
            string("adjustment-maintenance-confirmation-plan-graph/v2"),
        ),
        (
            "eligiblePredictionSetSha256",
            string(&eligible_prediction_set_sha256),
        ),
        ("entries", entry_values),
        ("family", string(registration.family.wire())),
        (
            "missingDueKeySetSha256",
            string(&missing_due_key_set_sha256),
        ),
        ("missingDueKeys", missing_values),
        (
            "registrationSha256",
            string(&registration.registration_sha256),
        ),
    ]);
    let plan_graph_sha256 = hash_value(wire, &plan_graph_unsigned).ok_or(Codec)?;
    let plan_graph = with_field(
        &plan_graph_unsigned,
        "planGraphSha256",
        string(&plan_graph_sha256),
    )
    .ok_or(Codec)?;
    let revision_catalog_watermark_sha256 = history_watermark(history, wire).ok_or(Codec)?;
    let snapshot_root_sha256 = hash_value(
        wire,
        &object(vec![
            (
                "eligiblePredictionSetSha256",
                string(&eligible_prediction_set_sha256),
            ),
            (
                "expectedKeySetSha256",
                string(&expected.reserved_key_sha256),
            ),
            (
                "missingDueKeySetSha256",
                string(&missing_due_key_set_sha256),
            ),
            ("targetCutoffAt", string(&shadow.target_cutoff_at)),
        ]),
    )
    .ok_or(Codec)?;
    Ok(object(vec![
        (
            "contractVersion",
            string("adjustment-maintenance-unsupported-confirmation-value-plan/v1"),
        ),
        (
            "eligiblePredictionSetSha256",
            string(&eligible_prediction_set_sha256),
        ),
        ("entryCount", JsonValue::Number(expected.entry_count as f64)),
        (
            "expectedKeySetSha256",
            string(&expected.reserved_key_sha256),
        ),
        ("family", string(registration.family.wire())),
        (
            "missingDueKeyCount",
            JsonValue::Number(missing_due_keys.len() as f64),
        ),
        (
            "missingDueKeySetSha256",
            string(&missing_due_key_set_sha256),
        ),
        ("planGraph", plan_graph),
        (
            "registrationSha256",
            string(&registration.registration_sha256),
        ),
        (
            "revisionCatalogWatermarkSha256",
            string(&revision_catalog_watermark_sha256),
        ),
        ("snapshotRootSha256", string(&snapshot_root_sha256)),
        ("state", string("unsupported_planned")),
        ("targetCutoffAt", string(&shadow.target_cutoff_at)),
    ]))
}

/// Project one value-blind capsule address using the source field names.
fn entry_value(entry: &ConfirmationEntry) -> JsonValue {
    object(vec![
        (
            "comparatorMemberSha256",
            string(&entry.comparator_member_sha256),
        ),
        ("dueKey", string(&entry.due_key)),
        ("graphManifestSha256", string(&entry.graph_manifest_sha256)),
        (
            "payloadIdentitySha256",
            string(&entry.payload_identity_sha256),
        ),
        (
            "predictionBodySha256",
            string(&entry.prediction_body_sha256),
        ),
        ("receiptSha256", string(&entry.receipt_sha256)),
        (
            "sourceProjectionSha256",
            string(&entry.source_projection_sha256),
        ),
        ("sourceReceiptSha256", string(&entry.source_receipt_sha256)),
    ])
}

/// Hash the source complete-frontier watermark without opening retained values.
fn history_watermark(
    history: &NativeMaintenanceHistoricalArchiveIndex,
    wire: &NativeWirePrimitives,
) -> Option<String> {
    hash_value(
        wire,
        &object(vec![
            (
                "archiveCommitOrdinal",
                string(&history.catalog.archive_commit_ordinal),
            ),
            (
                "catalogFrontierSha256",
                string(&history.catalog.frontier_root_sha256),
            ),
            ("historyRootSha256", string(&history.history_root_sha256)),
        ]),
    )
}

/// Project the source daily due object without altering its original retry cutoff.
fn due_value(due: &NativeMaintenanceDue) -> JsonValue {
    object(vec![
        ("dueKey", string(&due.due_key)),
        ("family", JsonValue::Null),
        ("mode", string(due.mode.as_str())),
        ("originalCutoffAt", string(&due.original_cutoff_at)),
        ("scope", string(&due.scope)),
    ])
}

/// Resolve one closed maintenance family spelling.
fn family(value: &str) -> Option<NativeMaintenanceCaptureFamily> {
    // reject aliases rather than normalizing caller material
    match value {
        "temperature" => Some(NativeMaintenanceCaptureFamily::Temperature),
        "wind" => Some(NativeMaintenanceCaptureFamily::Wind),
        "rain" => Some(NativeMaintenanceCaptureFamily::Rain),
        _ => None,
    }
}

/// Require the scheduler's exact nullable-family daily due identity.
fn valid_daily_due(due: &NativeMaintenanceDue, wire: &NativeWirePrimitives) -> bool {
    due.family.is_none()
        && due.mode == NativeMaintenanceDueMode::Daily
        && due.scope == "daily"
        && valid_daily_due_key(&due.due_key)
        && wire
            .canonical_utc_milliseconds(&due.original_cutoff_at)
            .is_some()
}

/// Recognize one exact source daily due key without a regex dependency at this boundary.
fn valid_daily_due_key(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 16
        && bytes.starts_with(b"daily/20")
        && bytes[6..10].iter().all(u8::is_ascii_digit)
        && bytes[10] == b'-'
        && bytes[11..13].iter().all(u8::is_ascii_digit)
        && bytes[13] == b'-'
        && bytes[14..16].iter().all(u8::is_ascii_digit)
}

/// Require one bounded decimal unsigned archive ordinal.
fn valid_archive_ordinal(value: &str) -> bool {
    (1..=20).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_digit())
}

/// Require one lowercase forty-hex Git commit.
fn valid_git_commit(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Require one immutable sha256-prefixed container image digest.
fn valid_image_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(valid_hash)
}

/// Require the installed immutable date-ordinal release grammar.
fn valid_release(value: &str) -> bool {
    let bytes = value.as_bytes();
    // retain the exact four-two-two date and positive unpadded ordinal spelling
    if bytes.len() < 12
        || !bytes[..4].iter().all(u8::is_ascii_digit)
        || bytes[4] != b'.'
        || !bytes[5..7].iter().all(u8::is_ascii_digit)
        || bytes[7] != b'.'
        || !bytes[8..10].iter().all(u8::is_ascii_digit)
        || bytes[10] != b'-'
    {
        return false;
    }
    let ordinal = &bytes[11..];
    !ordinal.is_empty()
        && ordinal[0] != b'0'
        && ordinal.len() <= 3
        && ordinal.iter().all(u8::is_ascii_digit)
}

/// Hash one canonical source value including the archive codec's final LF.
fn hash_value(wire: &NativeWirePrimitives, value: &JsonValue) -> Option<String> {
    Some(format_digest(wire.sha256(&wire.encode_sorted_json(value)?)))
}

/// Clone one object while excluding a single self-authenticating field.
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

/// Append one field to an already closed object before canonical sorted encoding.
fn with_field(value: &JsonValue, name: &str, field_value: JsonValue) -> Option<JsonValue> {
    let JsonValue::Object(fields) = value else {
        return None;
    };
    let mut fields = fields.clone();
    fields.push((name.encode_utf16().collect(), field_value));
    Some(JsonValue::Object(fields))
}
