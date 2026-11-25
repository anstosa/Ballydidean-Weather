//! Exact rain V3 monthly fit-input assembly over verified future-only history.

use std::cmp::Ordering;
use std::collections::{BTreeSet, HashMap};
use std::fmt::{self, Display, Formatter};

use weather_contracts::{
    AdjustmentRainFixedGaugeTargetProjection, AdjustmentRainGateControlProjectionRows,
    AdjustmentRainGateFeatureProjectionRows, AdjustmentRevisionCommitReceiptProjectionKindEnum,
    AdjustmentRevisionProjectionDocument, JsonValue, Nullable,
};
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};

use crate::maintenance_canonical_json::{field, text, valid_hash};
use crate::native_maintenance_historical_catalog::NativeMaintenanceHistoricalFamily;
use crate::native_maintenance_historical_fit::{
    NATIVE_MAINTENANCE_HISTORICAL_FIT_VERSION, NativeMaintenanceHistoricalFitMember,
    NativeMaintenanceHistoricalFitProjection,
};
use crate::native_maintenance_retirement_files_v2::format_digest;
use crate::worker_rain_persistence::fixed_gauge_weighted_median;
use crate::{
    ApiMaintenanceRevisionTransportDocument, NativeWirePrimitives,
    VerifiedNativeMaintenanceCaptureEpochWitness, decode_native_api_revision_document,
};

pub const NATIVE_MAINTENANCE_RAIN_MONTHLY_FIT_VERSION: &str =
    "adjustment-rain-monthly-fit-assembly/v1";
pub const NATIVE_MAINTENANCE_RAIN_DEVELOPMENT_POPULATION_VERSION: &str =
    "rain-maintenance-development-population/v3";

const FIT_INPUT_VERSION: &str = "rain-maintenance-fit-input/v3";
const FIXED_GAUGE_VERSION: &str = "adjustment-rain-fixed-gauge-target-projection/v1";
const RAIN_FEATURE_VERSION: &str = "adjustment-rain-gate-feature-projection/v2";
const RAIN_CONTROL_VERSION: &str = "adjustment-rain-gate-control-projection/v3";
const DAY_MILLISECONDS: i64 = 86_400_000;
const HOUR_MILLISECONDS: i64 = 3_600_000;
const MAXIMUM_ROWS: usize = 250_000;
const RETAINED_RAIN_ARTIFACT: &[u8] =
    include_bytes!("../../weather-contracts/retained-rain-hurdle-wind.json");

/// One value-blind source identity in the exact annual V3 population.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceRainDevelopmentMember {
    pub issued_at: String,
    pub key: String,
    pub model_lead_hours: u64,
    pub operational_horizon_hours: u64,
    pub phase_eligible: bool,
    pub source_member_sha256: String,
    pub source_receipt_sha256: String,
    pub target_available: bool,
    pub valid_at: String,
}

/// Complete annual source-population proof retained by the native fitter request.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceRainDevelopmentPopulation {
    pub contract_version: &'static str,
    pub cycle_hours: [u64; 4],
    pub development_end_at: String,
    pub development_start_at: String,
    pub eligible_row_count: usize,
    pub excluded_cold_row_count: usize,
    pub expected_row_count: usize,
    pub missing_source_row_count: usize,
    pub missing_target_row_count: usize,
    pub observed_row_count: usize,
    pub operational_horizon_hours: [u64; 23],
    pub population_member_root_sha256: String,
    pub population_receipt_root_sha256: String,
    pub population_sha256: String,
    pub source_model_lead_hours: [u64; 23],
    pub source_population: Vec<NativeMaintenanceRainDevelopmentMember>,
}

impl NativeMaintenanceRainDevelopmentPopulation {
    /// Project the exact predecessor proof value consumed by the fit sandbox.
    pub fn to_json(&self) -> JsonValue {
        object(vec![
            string_field("contractVersion", self.contract_version),
            array_number_field("cycleHours", &self.cycle_hours),
            string_field("developmentEndAt", &self.development_end_at),
            string_field("developmentStartAt", &self.development_start_at),
            number_field("eligibleRowCount", self.eligible_row_count as f64),
            number_field("excludedColdRowCount", self.excluded_cold_row_count as f64),
            number_field("expectedRowCount", self.expected_row_count as f64),
            number_field(
                "missingSourceRowCount",
                self.missing_source_row_count as f64,
            ),
            number_field(
                "missingTargetRowCount",
                self.missing_target_row_count as f64,
            ),
            number_field("observedRowCount", self.observed_row_count as f64),
            array_number_field("operationalHorizonHours", &self.operational_horizon_hours),
            string_field(
                "populationMemberRootSha256",
                &self.population_member_root_sha256,
            ),
            string_field(
                "populationReceiptRootSha256",
                &self.population_receipt_root_sha256,
            ),
            string_field("populationSha256", &self.population_sha256),
            array_number_field("sourceModelLeadHours", &self.source_model_lead_hours),
            (
                "sourcePopulation".encode_utf16().collect(),
                JsonValue::Array(
                    self.source_population
                        .iter()
                        .map(NativeMaintenanceRainDevelopmentMember::to_json)
                        .collect(),
                ),
            ),
        ])
    }
}

impl NativeMaintenanceRainDevelopmentMember {
    /// Project one compact member with the source's exact field set.
    fn to_json(&self) -> JsonValue {
        object(vec![
            string_field("issuedAt", &self.issued_at),
            string_field("key", &self.key),
            number_field("modelLeadHours", self.model_lead_hours as f64),
            number_field(
                "operationalHorizonHours",
                self.operational_horizon_hours as f64,
            ),
            boolean_field("phaseEligible", self.phase_eligible),
            string_field("sourceMemberSha256", &self.source_member_sha256),
            string_field("sourceReceiptSha256", &self.source_receipt_sha256),
            boolean_field("targetAvailable", self.target_available),
            string_field("validAt", &self.valid_at),
        ])
    }
}

/// Genuine source incompleteness state returned before running the native fitter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceRainMonthlyNoCandidateReason {
    IncompleteDevelopmentPopulation,
    IncompleteDevelopmentTarget,
}

impl NativeMaintenanceRainMonthlyNoCandidateReason {
    /// Return the exact predecessor reason spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::IncompleteDevelopmentPopulation => "incomplete_development_population",
            Self::IncompleteDevelopmentTarget => "incomplete_development_target",
        }
    }
}

/// Exact common rain assembly identities retained by both terminal states.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceRainMonthlyFitCommon {
    pub contract_version: &'static str,
    pub cutoff_at: String,
    pub development_end_at: String,
    pub development_population: NativeMaintenanceRainDevelopmentPopulation,
    pub development_start_at: String,
    pub due_month: String,
    pub epoch_witness_sha256: String,
    pub family: NativeMaintenanceHistoricalFamily,
    pub historical_member_root_sha256: String,
    pub source_member_root_sha256: String,
    pub source_receipt_root_sha256: String,
}

/// Ready rain fit request and its exact canonical input identity.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceRainMonthlyFitAssembly {
    pub common: NativeMaintenanceRainMonthlyFitCommon,
    pub fit_input: JsonValue,
    pub input_manifest_sha256: String,
}

/// Closed rain monthly result preserving both truthful terminal shapes.
#[derive(Clone, Debug, PartialEq)]
pub enum NativeMaintenanceRainMonthlyFitOutcome {
    NoCandidate {
        common: NativeMaintenanceRainMonthlyFitCommon,
        reason: NativeMaintenanceRainMonthlyNoCandidateReason,
    },
    Ready(NativeMaintenanceRainMonthlyFitAssembly),
}

impl NativeMaintenanceRainMonthlyFitOutcome {
    /// Project the exact ready or noncandidate predecessor object.
    pub fn to_json(&self) -> JsonValue {
        // preserve the two disjoint source field sets without nullable placeholders
        match self {
            Self::NoCandidate { common, reason } => {
                let mut fields = common.json_fields();
                fields.push(string_field("reason", reason.as_str()));
                fields.push(string_field("state", "no_candidate"));
                object(fields)
            }
            Self::Ready(assembly) => {
                let mut fields = assembly.common.json_fields();
                fields.push((
                    "fitInput".encode_utf16().collect(),
                    assembly.fit_input.clone(),
                ));
                fields.push(string_field(
                    "inputManifestSha256",
                    &assembly.input_manifest_sha256,
                ));
                fields.push(string_field("state", "ready"));
                object(fields)
            }
        }
    }
}

impl NativeMaintenanceRainMonthlyFitCommon {
    /// Build the shared predecessor fields in their original insertion order.
    fn json_fields(&self) -> Vec<(Vec<u16>, JsonValue)> {
        vec![
            string_field("contractVersion", self.contract_version),
            string_field("cutoffAt", &self.cutoff_at),
            string_field("developmentEndAt", &self.development_end_at),
            (
                "developmentPopulation".encode_utf16().collect(),
                self.development_population.to_json(),
            ),
            string_field("developmentStartAt", &self.development_start_at),
            string_field("dueMonth", &self.due_month),
            string_field("epochWitnessSha256", &self.epoch_witness_sha256),
            string_field("family", self.family.as_str()),
            string_field(
                "historicalMemberRootSha256",
                &self.historical_member_root_sha256,
            ),
            string_field("sourceMemberRootSha256", &self.source_member_root_sha256),
            string_field("sourceReceiptRootSha256", &self.source_receipt_root_sha256),
        ]
    }
}

/// Closed rain assembly admission and lineage failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceRainMonthlyFitError {
    Input,
    HistoricalProjection,
    Clock,
    Member,
    Receipt,
    Target,
    Feature,
    Population,
    Numeric,
    Size,
    Codec,
}

impl Display for NativeMaintenanceRainMonthlyFitError {
    /// Render one bounded diagnostic without private fit material.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Input => "rain monthly fit input is invalid",
            Self::HistoricalProjection => "rain monthly historical projection differs",
            Self::Clock => "rain monthly causal clock differs",
            Self::Member => "rain monthly historical member differs",
            Self::Receipt => "rain monthly historical receipt differs",
            Self::Target => "rain monthly fixed-gauge target differs",
            Self::Feature => "rain monthly forecast feature differs",
            Self::Population => "rain monthly development population differs",
            Self::Numeric => "rain monthly numeric value is invalid",
            Self::Size => "rain monthly row ceiling exceeded",
            Self::Codec => "rain monthly canonical encoding failed",
        })
    }
}

impl std::error::Error for NativeMaintenanceRainMonthlyFitError {}

/// Assemble one authenticated future-only V3 rain fit request.
pub fn build_native_adjustment_rain_monthly_fit_assembly(
    due_month: &str,
    epoch_witness: &VerifiedNativeMaintenanceCaptureEpochWitness,
    historical: &NativeMaintenanceHistoricalFitProjection,
) -> Result<NativeMaintenanceRainMonthlyFitOutcome, NativeMaintenanceRainMonthlyFitError> {
    use NativeMaintenanceRainMonthlyFitError as Error;

    // admit the exact rain-only request and verified lifecycle identity
    if !valid_month(due_month) || !valid_hash(&epoch_witness.witness_sha256) {
        return Err(Error::Input);
    }
    validate_historical_projection(due_month, epoch_witness, historical)?;
    let assembled = build_historical_rows(epoch_witness, historical)?;
    let population =
        build_development_population(due_month, assembled.population_members, &assembled.rows)?;
    let common = NativeMaintenanceRainMonthlyFitCommon {
        contract_version: NATIVE_MAINTENANCE_RAIN_MONTHLY_FIT_VERSION,
        cutoff_at: historical.cutoff_at.clone(),
        development_end_at: population.development_end_at.clone(),
        development_start_at: population.development_start_at.clone(),
        development_population: population,
        due_month: due_month.to_owned(),
        epoch_witness_sha256: epoch_witness.witness_sha256.clone(),
        family: NativeMaintenanceHistoricalFamily::Rain,
        historical_member_root_sha256: historical.member_root_sha256.clone(),
        source_member_root_sha256: assembled.source_member_root_sha256,
        source_receipt_root_sha256: assembled.source_receipt_root_sha256,
    };
    // return an honest source-population gap before constructing fit material
    if common.development_population.missing_source_row_count > 0 {
        return Ok(NativeMaintenanceRainMonthlyFitOutcome::NoCandidate {
            common,
            reason: NativeMaintenanceRainMonthlyNoCandidateReason::IncompleteDevelopmentPopulation,
        });
    }
    // distinguish unavailable genuine targets from absent source features
    if common.development_population.missing_target_row_count > 0 {
        return Ok(NativeMaintenanceRainMonthlyFitOutcome::NoCandidate {
            common,
            reason: NativeMaintenanceRainMonthlyNoCandidateReason::IncompleteDevelopmentTarget,
        });
    }
    let development_keys: BTreeSet<&str> = common
        .development_population
        .source_population
        .iter()
        .filter(|member| member.phase_eligible && member.target_available)
        .map(|member| member.key.as_str())
        .collect();
    let feature_names = retained_feature_names()?;
    let fit_input = object(vec![
        string_field("contractVersion", FIT_INPUT_VERSION),
        (
            "developmentPopulation".encode_utf16().collect(),
            common.development_population.to_json(),
        ),
        (
            "developmentRows".encode_utf16().collect(),
            JsonValue::Array(
                assembled
                    .rows
                    .iter()
                    .filter(|row| development_keys.contains(row.key.as_str()))
                    .map(|row| row.value.clone())
                    .collect(),
            ),
        ),
        ("featureNames".encode_utf16().collect(), feature_names),
        string_field("month", due_month),
        (
            "trainingRows".encode_utf16().collect(),
            JsonValue::Array(
                assembled
                    .rows
                    .iter()
                    .filter(|row| row.valid_at < common.development_start_at)
                    .map(|row| row.value.clone())
                    .collect(),
            ),
        ),
    ]);
    let input_manifest_sha256 = hash_json(&fit_input)?;
    Ok(NativeMaintenanceRainMonthlyFitOutcome::Ready(
        NativeMaintenanceRainMonthlyFitAssembly {
            common,
            fit_input,
            input_manifest_sha256,
        },
    ))
}

#[derive(Clone, Debug, PartialEq)]
struct NumericalFeatureRow {
    features: Vec<Option<f64>>,
    raw: f64,
    raw_target_hour_temperature_c: f64,
}

#[derive(Clone)]
struct FeatureMember {
    historical_index: usize,
    logical_received_at: String,
    numerical: NumericalFeatureRow,
    run_initialized_at: String,
    valid_at: String,
    model_lead_hours: u64,
}

#[derive(Clone, Copy, PartialEq)]
struct TargetActual {
    gauge_count: usize,
    target: f64,
}

struct TargetGroup {
    actual: Option<TargetActual>,
    last_ordinal: String,
    member_indices: Vec<usize>,
    valid_at: String,
}

struct SelectedTarget {
    actual: Option<TargetActual>,
    last_ordinal: String,
    member_indices: Vec<usize>,
}

struct HistoricalRainRow {
    key: String,
    valid_at: String,
    value: JsonValue,
}

struct HistoricalRows {
    population_members: Vec<NativeMaintenanceRainDevelopmentMember>,
    rows: Vec<HistoricalRainRow>,
    source_member_root_sha256: String,
    source_receipt_root_sha256: String,
}

/// Join authenticated feature revisions to the first genuine twelve-gauge target.
/// Exact authenticated historical rows shared by development fitting and pre-month controls.
pub(crate) struct NativeMaintenanceRainHistoricalRows {
    pub rows: Vec<JsonValue>,
    pub source_member_root_sha256: String,
    pub source_receipt_root_sha256: String,
}

/// Retain the existing causal join without imposing development-fit support thresholds.
pub(crate) fn build_native_adjustment_rain_historical_rows(
    witness: &VerifiedNativeMaintenanceCaptureEpochWitness,
    historical: &NativeMaintenanceHistoricalFitProjection,
) -> Result<NativeMaintenanceRainHistoricalRows, NativeMaintenanceRainMonthlyFitError> {
    let assembled = build_historical_rows(witness, historical)?;
    Ok(NativeMaintenanceRainHistoricalRows {
        rows: assembled.rows.into_iter().map(|row| row.value).collect(),
        source_member_root_sha256: assembled.source_member_root_sha256,
        source_receipt_root_sha256: assembled.source_receipt_root_sha256,
    })
}

fn build_historical_rows(
    epoch_witness: &VerifiedNativeMaintenanceCaptureEpochWitness,
    historical: &NativeMaintenanceHistoricalFitProjection,
) -> Result<HistoricalRows, NativeMaintenanceRainMonthlyFitError> {
    use NativeMaintenanceRainMonthlyFitError as Error;

    let wire = NativeWirePrimitives;
    let epoch = canonical_instant(&wire, &epoch_witness.epoch_at)?;
    let cutoff = canonical_instant(&wire, &historical.cutoff_at)?;
    let mut groups = Vec::<TargetGroup>::new();
    let mut group_indices = HashMap::<String, usize>::new();
    let mut features = Vec::<FeatureMember>::new();
    // replay every relevant archived member through the canonical production decoder
    for (historical_index, member) in historical.projection_members.iter().enumerate() {
        let fixed =
            text(&member.document, "contractVersion").as_deref() == Some(FIXED_GAUGE_VERSION);
        let feature = matches!(
            text(&member.document, "contractVersion").as_deref(),
            Some(RAIN_FEATURE_VERSION | RAIN_CONTROL_VERSION)
        );
        // omit unrelated classes exactly as the predecessor join does
        if !fixed && !feature {
            continue;
        }
        validate_member(&wire, member, epoch, cutoff)?;
        let decoded =
            decode_native_api_revision_document(&member.payload_bytes).ok_or(Error::Member)?;
        let decoded_json = wire
            .decode_json(&member.payload_bytes)
            .ok_or(Error::Member)?;
        // bind the decoded document and selected row to immutable archived bytes
        if decoded_json != member.document
            || text(&decoded_json, "projectionKind").as_deref()
                != Some(member.projection_kind.as_str())
            || json_rows(&decoded_json).and_then(|rows| rows.get(member.row_index))
                != Some(&member.row)
        {
            return Err(Error::Member);
        }
        // retain fixed-gauge groups atomically by their whole payload identity
        if fixed {
            let ApiMaintenanceRevisionTransportDocument::FixedGauge(document) = decoded else {
                return Err(Error::Target);
            };
            validate_fixed_gauge_clocks(&wire, &document, member, epoch, cutoff)?;
            let actual = fixed_gauge_actual(&document)?;
            let index = match group_indices.get(&member.payload_identity_sha256).copied() {
                Some(index) => index,
                None => {
                    let index = groups.len();
                    groups.push(TargetGroup {
                        actual,
                        last_ordinal: member.receipt.archiveCommitOrdinal.clone(),
                        member_indices: Vec::new(),
                        valid_at: document.validAt.clone(),
                    });
                    group_indices.insert(member.payload_identity_sha256.clone(), index);
                    index
                }
            };
            let group = groups.get_mut(index).ok_or(Error::Target)?;
            // require all rows from one payload to agree on decoded target and valid clock
            if group.actual != actual || group.valid_at != document.validAt {
                return Err(Error::Target);
            }
            if compare_ordinal(&member.receipt.archiveCommitOrdinal, &group.last_ordinal)
                == Ordering::Greater
            {
                group.last_ordinal = member.receipt.archiveCommitOrdinal.clone();
            }
            group.member_indices.push(historical_index);
            continue;
        }
        let ApiMaintenanceRevisionTransportDocument::Standard(document) = decoded else {
            return Err(Error::Feature);
        };
        let feature = feature_member_from_document(member, historical_index, document.as_ref())?;
        features.push(feature);
    }
    let mut targets = HashMap::<String, SelectedTarget>::new();
    // admit only complete twelve-row target groups with distinct native receipts
    for group in groups {
        let mut member_indices = group.member_indices;
        member_indices.sort_by_key(|index| historical.projection_members[*index].row_index);
        let complete = member_indices.len() == 12
            && member_indices
                .iter()
                .enumerate()
                .all(|(row, index)| historical.projection_members[*index].row_index == row)
            && member_indices
                .iter()
                .map(|index| {
                    historical.projection_members[*index]
                        .receipt
                        .receiptSha256
                        .as_str()
                })
                .collect::<BTreeSet<_>>()
                .len()
                == 12;
        // reject partial or repeated source populations rather than selecting a subset
        if !complete {
            return Err(Error::Target);
        }
        let selected = SelectedTarget {
            actual: group.actual,
            last_ordinal: group.last_ordinal,
            member_indices,
        };
        // freeze the first native group at the lowest last commit ordinal
        match targets.get(&group.valid_at) {
            Some(previous)
                if compare_ordinal(&selected.last_ordinal, &previous.last_ordinal)
                    != Ordering::Less => {}
            _ => {
                targets.insert(group.valid_at, selected);
            }
        }
    }
    features.sort_by(|left, right| {
        compare_ordinal(
            &historical.projection_members[left.historical_index]
                .receipt
                .archiveCommitOrdinal,
            &historical.projection_members[right.historical_index]
                .receipt
                .archiveCommitOrdinal,
        )
        .then_with(|| {
            historical.projection_members[left.historical_index]
                .row_index
                .cmp(&historical.projection_members[right.historical_index].row_index)
        })
    });
    let mut seen = HashMap::<String, NumericalFeatureRow>::new();
    let mut population_members = Vec::new();
    let mut rows = Vec::new();
    let mut consumed_members = BTreeSet::<String>::new();
    let mut consumed_receipts = BTreeSet::<String>::new();
    // retain the first pre-target feature revision for each run and target hour
    for feature in features {
        let member = &historical.projection_members[feature.historical_index];
        let run = canonical_instant(&wire, &feature.run_initialized_at)?;
        let valid = canonical_instant(&wire, &feature.valid_at)?;
        let received = canonical_instant(&wire, &feature.logical_received_at)?;
        let issued = run.checked_add(8 * HOUR_MILLISECONDS).ok_or(Error::Clock)?;
        causal(run, epoch, cutoff)?;
        causal(received, epoch, cutoff)?;
        causal(valid, epoch, cutoff)?;
        let identity = format!("{}/{}", feature.run_initialized_at, feature.valid_at);
        let operational_horizon_hours = feature
            .model_lead_hours
            .checked_sub(8)
            .ok_or(Error::Feature)?;
        let receipt_at = canonical_instant(&wire, &member.receipt.archiveCommittedAt)?;
        // exclude revisions that were unavailable at their target decision
        if receipt_at >= valid
            || received >= valid
            || valid.checked_sub(run)
                != feature
                    .model_lead_hours
                    .checked_mul(HOUR_MILLISECONDS as u64)
                    .and_then(|value| i64::try_from(value).ok())
            || received < issued
            || receipt_at < received
            || !(1..=23).contains(&operational_horizon_hours)
        {
            continue;
        }
        // reject contradictory revisions under one forecast key
        if let Some(previous) = seen.get(&identity) {
            if previous != &feature.numerical {
                return Err(Error::Feature);
            }
            continue;
        }
        seen.insert(identity.clone(), feature.numerical.clone());
        let target = targets.get(&feature.valid_at);
        let phase_eligible = feature.numerical.raw_target_hour_temperature_c > 2.0;
        let issued_at = wire.format_utc_milliseconds(issued).ok_or(Error::Clock)?;
        population_members.push(NativeMaintenanceRainDevelopmentMember {
            issued_at: issued_at.clone(),
            key: identity.clone(),
            model_lead_hours: feature.model_lead_hours,
            operational_horizon_hours,
            phase_eligible,
            source_member_sha256: member.member_sha256.clone(),
            source_receipt_sha256: member.receipt.receiptSha256.clone(),
            target_available: target.and_then(|target| target.actual).is_some(),
            valid_at: feature.valid_at.clone(),
        });
        // cold-phase rows remain value-blind population evidence only
        if !phase_eligible {
            continue;
        }
        let Some((target, actual)) =
            target.and_then(|target| target.actual.map(|actual| (target, actual)))
        else {
            continue;
        };
        let persistence_at = issued.checked_sub(HOUR_MILLISECONDS).ok_or(Error::Clock)?;
        let persistence_key = wire
            .format_utc_milliseconds(persistence_at)
            .ok_or(Error::Clock)?;
        let persistence = targets.get(&persistence_key).filter(|candidate| {
            candidate.member_indices.iter().all(|index| {
                historical.projection_members[*index]
                    .receipt
                    .archiveCommittedAt
                    < issued_at
            })
        });
        let target_max_receipt_at = target
            .member_indices
            .iter()
            .map(|index| {
                historical.projection_members[*index]
                    .receipt
                    .archiveCommittedAt
                    .as_str()
            })
            .max()
            .unwrap_or(epoch_witness.epoch_at.as_str());
        let mut target_members = target
            .member_indices
            .iter()
            .map(|index| historical.projection_members[*index].member_sha256.clone())
            .collect::<Vec<_>>();
        target_members.sort();
        let target_row_sha256 = hash_json(&JsonValue::Array(
            target_members.iter().map(|value| string(value)).collect(),
        ))?;
        let row = historical_rain_row(
            &identity,
            &feature,
            member,
            actual,
            persistence.and_then(|candidate| candidate.actual),
            target_max_receipt_at,
            &target_row_sha256,
        );
        rows.push(row);
        // enforce the independent native matrix ceiling
        if rows.len() > MAXIMUM_ROWS {
            return Err(Error::Size);
        }
        retain_consumed(member, &mut consumed_members, &mut consumed_receipts);
        // bind all twelve target rows, including explicit missing gauges
        for index in &target.member_indices {
            retain_consumed(
                &historical.projection_members[*index],
                &mut consumed_members,
                &mut consumed_receipts,
            );
        }
        // retain the causal persistence population when it was genuinely available
        if let Some(persistence) = persistence {
            for index in &persistence.member_indices {
                retain_consumed(
                    &historical.projection_members[*index],
                    &mut consumed_members,
                    &mut consumed_receipts,
                );
            }
        }
    }
    rows.sort_by(|left, right| {
        left.valid_at
            .cmp(&right.valid_at)
            .then_with(|| left.key.cmp(&right.key))
    });
    population_members.sort_by(|left, right| {
        left.valid_at
            .cmp(&right.valid_at)
            .then_with(|| left.key.cmp(&right.key))
    });
    Ok(HistoricalRows {
        population_members,
        rows,
        source_member_root_sha256: hash_strings(consumed_members)?,
        source_receipt_root_sha256: hash_strings(consumed_receipts)?,
    })
}

/// Derive one feature member from either admitted source projection variant.
fn feature_member_from_document(
    member: &NativeMaintenanceHistoricalFitMember,
    historical_index: usize,
    document: &AdjustmentRevisionProjectionDocument,
) -> Result<FeatureMember, NativeMaintenanceRainMonthlyFitError> {
    use NativeMaintenanceRainMonthlyFitError::Feature;

    match document {
        AdjustmentRevisionProjectionDocument::RainFeature(document) => {
            let row = document.rows.get(member.row_index).ok_or(Feature)?;
            feature_member(
                historical_index,
                &document.logicalKey.runInitializedAt,
                &document.logicalReceivedAt,
                row,
            )
        }
        AdjustmentRevisionProjectionDocument::RainControl(document) => {
            let row = document.rows.get(member.row_index).ok_or(Feature)?;
            control_feature_member(
                historical_index,
                &document.logicalKey.runInitializedAt,
                &document.logicalReceivedAt,
                row,
            )
        }
        _ => Err(Feature),
    }
}

/// Decode one canonical feature row while preserving null and signed-zero semantics.
fn feature_member(
    historical_index: usize,
    run_initialized_at: &str,
    logical_received_at: &str,
    row: &AdjustmentRainGateFeatureProjectionRows,
) -> Result<FeatureMember, NativeMaintenanceRainMonthlyFitError> {
    Ok(FeatureMember {
        historical_index,
        logical_received_at: logical_received_at.to_owned(),
        numerical: numerical_feature_row(
            &row.features64,
            &row.rawPrecipitationMm64,
            &row.rawTargetHourTemperatureC64,
        )?,
        run_initialized_at: run_initialized_at.to_owned(),
        valid_at: row.validAt.clone(),
        model_lead_hours: row.modelLeadHours,
    })
}

/// Decode the feature subset retained by a control projection.
fn control_feature_member(
    historical_index: usize,
    run_initialized_at: &str,
    logical_received_at: &str,
    row: &AdjustmentRainGateControlProjectionRows,
) -> Result<FeatureMember, NativeMaintenanceRainMonthlyFitError> {
    Ok(FeatureMember {
        historical_index,
        logical_received_at: logical_received_at.to_owned(),
        numerical: numerical_feature_row(
            &row.features64,
            &row.rawPrecipitationMm64,
            &row.rawTargetHourTemperatureC64,
        )?,
        run_initialized_at: run_initialized_at.to_owned(),
        valid_at: row.validAt.clone(),
        model_lead_hours: row.modelLeadHours,
    })
}

/// Decode exact binary64 forecast values without accepting infinities or malformed bits.
fn numerical_feature_row(
    features: &[Nullable<String>],
    raw: &str,
    temperature: &str,
) -> Result<NumericalFeatureRow, NativeMaintenanceRainMonthlyFitError> {
    let features = features
        .iter()
        .map(|value| match value {
            Nullable::Null => Ok(None),
            Nullable::Value(value) => decode_binary64(value).map(Some),
        })
        .collect::<Result<Vec<_>, _>>()?;
    // retain only the frozen 107-feature geometry
    if features.len() != 107 {
        return Err(NativeMaintenanceRainMonthlyFitError::Feature);
    }
    Ok(NumericalFeatureRow {
        features,
        raw: decode_binary64(raw)?,
        raw_target_hour_temperature_c: decode_binary64(temperature)?,
    })
}

/// Build one exact numerical row in the fit sandbox's contract.
fn historical_rain_row(
    key: &str,
    feature: &FeatureMember,
    member: &NativeMaintenanceHistoricalFitMember,
    actual: TargetActual,
    persistence: Option<TargetActual>,
    target_max_receipt_at: &str,
    target_row_sha256: &str,
) -> HistoricalRainRow {
    let value = object(vec![
        number_field("actual", normalize_zero(actual.target)),
        string_field("evidenceClass", "development"),
        (
            "features".encode_utf16().collect(),
            JsonValue::Array(
                feature
                    .numerical
                    .features
                    .iter()
                    .map(|value| {
                        value
                            .map(|value| JsonValue::Number(normalize_zero(value)))
                            .unwrap_or(JsonValue::Null)
                    })
                    .collect(),
            ),
        ),
        number_field("gaugeCount", actual.gauge_count as f64),
        string_field("key", key),
        number_field("modelLeadHours", feature.model_lead_hours as f64),
        number_field(
            "operationalHorizonHours",
            feature.model_lead_hours.saturating_sub(8) as f64,
        ),
        (
            "persistencePrediction".encode_utf16().collect(),
            persistence
                .map(|value| JsonValue::Number(normalize_zero(value.target)))
                .unwrap_or(JsonValue::Null),
        ),
        number_field("raw", normalize_zero(feature.numerical.raw)),
        number_field(
            "rawTargetHourTemperatureC",
            normalize_zero(feature.numerical.raw_target_hour_temperature_c),
        ),
        string_field("runInitializedAt", &feature.run_initialized_at),
        string_field("sourceReceiptAt", &member.receipt.archiveCommittedAt),
        string_field("sourceRowSha256", &member.member_sha256),
        string_field("targetMaxReceiptAt", target_max_receipt_at),
        string_field("targetRowSha256", target_row_sha256),
        string_field("validAt", &feature.valid_at),
    ]);
    HistoricalRainRow {
        key: key.to_owned(),
        valid_at: feature.valid_at.clone(),
        value,
    }
}

/// Derive the weighted target only from parser-authenticated station rows.
fn fixed_gauge_actual(
    document: &AdjustmentRainFixedGaugeTargetProjection,
) -> Result<Option<TargetActual>, NativeMaintenanceRainMonthlyFitError> {
    let mut gauges = document
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
    let gauge_count = gauges.len();
    Ok(
        fixed_gauge_weighted_median(&mut gauges).map(|target| TargetActual {
            gauge_count,
            target,
        }),
    )
}

/// Bind fixed-gauge raw capture and interval clocks to the verified lifecycle range.
fn validate_fixed_gauge_clocks(
    wire: &NativeWirePrimitives,
    document: &AdjustmentRainFixedGaugeTargetProjection,
    member: &NativeMaintenanceHistoricalFitMember,
    epoch: i64,
    cutoff: i64,
) -> Result<(), NativeMaintenanceRainMonthlyFitError> {
    causal(
        canonical_instant(wire, &document.logicalReceivedAt)?,
        epoch,
        cutoff,
    )?;
    causal(canonical_instant(wire, &document.validAt)?, epoch, cutoff)?;
    // retain all raw claim clocks inside the authenticated interval
    for body in &document.captureBodies {
        for claim in &body.claims {
            causal(canonical_instant(wire, &claim.completedAt)?, epoch, cutoff)?;
            causal(canonical_instant(wire, &claim.windowStart)?, epoch, cutoff)?;
            causal(
                canonical_instant(wire, &claim.windowEndExclusive)?,
                epoch,
                cutoff,
            )?;
        }
    }
    let row = document
        .rows
        .get(member.row_index)
        .ok_or(NativeMaintenanceRainMonthlyFitError::Target)?;
    // retain every selected physical interval clock inside the same range
    for interval in &row.intervals {
        causal(canonical_instant(wire, &interval.validAt)?, epoch, cutoff)?;
        causal(
            canonical_instant(wire, &interval.completedAt)?,
            epoch,
            cutoff,
        )?;
    }
    Ok(())
}

/// Revalidate the native five-field member identity and receipt binding.
fn validate_member(
    wire: &NativeWirePrimitives,
    member: &NativeMaintenanceHistoricalFitMember,
    epoch: i64,
    cutoff: i64,
) -> Result<(), NativeMaintenanceRainMonthlyFitError> {
    use NativeMaintenanceRainMonthlyFitError::{Member, Receipt};

    let receipt = &member.receipt;
    // refuse detached bytes, receipts, ordinals, and row indices
    if digest(wire, &member.payload_bytes) != member.payload_identity_sha256
        || receipt.projectionIdentitySha256 != member.payload_identity_sha256
        || receipt.projectionSha256 != member.payload_identity_sha256
        || receipt_kind(receipt.projectionKind) != member.projection_kind
        || !valid_hash(&receipt.receiptSha256)
        || !valid_positive_ordinal(&receipt.archiveCommitOrdinal)
    {
        return Err(Receipt);
    }
    let identity = object(vec![
        string_field("archiveCommitOrdinal", &receipt.archiveCommitOrdinal),
        string_field("payloadIdentitySha256", &member.payload_identity_sha256),
        string_field("projectionKind", &member.projection_kind),
        string_field("receiptSha256", &receipt.receiptSha256),
        number_field("rowIndex", member.row_index as f64),
    ]);
    // rederive the archived row identity rather than trusting mutable metadata
    if hash_json(&identity)? != member.member_sha256 {
        return Err(Member);
    }
    causal(
        canonical_instant(wire, &receipt.archiveCommittedAt)?,
        epoch,
        cutoff,
    )
}

/// Build one full UTC year ending seven days before the due month.
fn development_interval(
    due_month: &str,
) -> Result<(String, String), NativeMaintenanceRainMonthlyFitError> {
    let wire = NativeWirePrimitives;
    let month_start = canonical_instant(&wire, &format!("{due_month}-01T00:00:00.000Z"))?;
    let end = month_start
        .checked_sub(7 * DAY_MILLISECONDS)
        .ok_or(NativeMaintenanceRainMonthlyFitError::Clock)?;
    let development_end_at = wire
        .format_utc_milliseconds(end)
        .ok_or(NativeMaintenanceRainMonthlyFitError::Clock)?;
    let year = development_end_at
        .get(..4)
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|year| *year > 0)
        .ok_or(NativeMaintenanceRainMonthlyFitError::Clock)?;
    let development_start_at = format!("{:04}{}", year - 1, &development_end_at[4..]);
    canonical_instant(&wire, &development_start_at)?;
    Ok((development_start_at, development_end_at))
}

#[derive(Clone)]
struct ExpectedPopulationMember {
    issued_at: String,
    key: String,
    model_lead_hours: u64,
    operational_horizon_hours: u64,
    valid_at: String,
}

/// Enumerate every source cycle and lead whose target lies in the annual interval.
fn expected_population(
    start_at: &str,
    end_at: &str,
) -> Result<Vec<ExpectedPopulationMember>, NativeMaintenanceRainMonthlyFitError> {
    let wire = NativeWirePrimitives;
    let start = canonical_instant(&wire, start_at)?;
    let end = canonical_instant(&wire, end_at)?;
    let first_run = start
        .checked_sub(31 * HOUR_MILLISECONDS)
        .ok_or(NativeMaintenanceRainMonthlyFitError::Clock)?
        .div_euclid(6 * HOUR_MILLISECONDS)
        * 6
        * HOUR_MILLISECONDS;
    let last_run = end
        .checked_sub(9 * HOUR_MILLISECONDS)
        .ok_or(NativeMaintenanceRainMonthlyFitError::Clock)?;
    let mut expected = Vec::new();
    let mut run = first_run;
    // preserve the issuance halo at both annual boundaries
    while run <= last_run {
        let run_at = wire
            .format_utc_milliseconds(run)
            .ok_or(NativeMaintenanceRainMonthlyFitError::Clock)?;
        let issued_at = wire
            .format_utc_milliseconds(
                run.checked_add(8 * HOUR_MILLISECONDS)
                    .ok_or(NativeMaintenanceRainMonthlyFitError::Clock)?,
            )
            .ok_or(NativeMaintenanceRainMonthlyFitError::Clock)?;
        // retain the frozen source leads in their original order
        for model_lead_hours in 9u64..=31 {
            let valid = run
                .checked_add(
                    i64::try_from(model_lead_hours)
                        .map_err(|_| NativeMaintenanceRainMonthlyFitError::Clock)?
                        * HOUR_MILLISECONDS,
                )
                .ok_or(NativeMaintenanceRainMonthlyFitError::Clock)?;
            // omit only targets outside the half-open development year
            if valid < start || valid >= end {
                continue;
            }
            let valid_at = wire
                .format_utc_milliseconds(valid)
                .ok_or(NativeMaintenanceRainMonthlyFitError::Clock)?;
            expected.push(ExpectedPopulationMember {
                issued_at: issued_at.clone(),
                key: format!("{run_at}/{valid_at}"),
                model_lead_hours,
                operational_horizon_hours: model_lead_hours - 8,
                valid_at,
            });
        }
        run = run
            .checked_add(6 * HOUR_MILLISECONDS)
            .ok_or(NativeMaintenanceRainMonthlyFitError::Clock)?;
    }
    Ok(expected)
}

/// Build the exact value-blind annual V3 source-population proof.
fn build_development_population(
    due_month: &str,
    population_members: Vec<NativeMaintenanceRainDevelopmentMember>,
    rows: &[HistoricalRainRow],
) -> Result<NativeMaintenanceRainDevelopmentPopulation, NativeMaintenanceRainMonthlyFitError> {
    use NativeMaintenanceRainMonthlyFitError as Error;

    let (development_start_at, development_end_at) = development_interval(due_month)?;
    let expected = expected_population(&development_start_at, &development_end_at)?;
    let mut observed = HashMap::<String, NativeMaintenanceRainDevelopmentMember>::new();
    // reject contradictory feature revisions under one population key
    for member in population_members {
        validate_population_member(&member)?;
        if let Some(previous) = observed.get(&member.key)
            && previous != &member
        {
            return Err(Error::Population);
        }
        observed.insert(member.key.clone(), member);
    }
    let mut source_population = Vec::new();
    let mut missing_source_row_count = 0usize;
    let mut missing_target_row_count = 0usize;
    // preserve expected cycle and lead order without inspecting target values
    for expected_member in &expected {
        let Some(member) = observed.get(&expected_member.key) else {
            missing_source_row_count += 1;
            continue;
        };
        // require exact source geometry before retaining the member
        if member.issued_at != expected_member.issued_at
            || member.model_lead_hours != expected_member.model_lead_hours
            || member.operational_horizon_hours != expected_member.operational_horizon_hours
            || member.valid_at != expected_member.valid_at
        {
            return Err(Error::Population);
        }
        source_population.push(member.clone());
        // count only source-eligible rows whose genuine target is unavailable
        if member.phase_eligible && !member.target_available {
            missing_target_row_count += 1;
        }
    }
    let mut rows_by_key = BTreeSet::<&str>::new();
    // prohibit duplicate joined target rows
    for row in rows {
        if !rows_by_key.insert(row.key.as_str()) {
            return Err(Error::Population);
        }
    }
    // prevent target metadata from concealing an absent numerical row
    for member in &source_population {
        if member.phase_eligible
            && member.target_available
            && !rows_by_key.contains(member.key.as_str())
        {
            missing_target_row_count += 1;
        }
    }
    let eligible_row_count = source_population
        .iter()
        .filter(|member| member.phase_eligible)
        .count();
    let excluded_cold_row_count = source_population.len() - eligible_row_count;
    let mut member_hashes = source_population
        .iter()
        .map(|member| member.source_member_sha256.clone())
        .collect::<Vec<_>>();
    member_hashes.sort();
    let receipt_hashes = source_population
        .iter()
        .map(|member| member.source_receipt_sha256.clone())
        .collect::<BTreeSet<_>>();
    let source_population_json = JsonValue::Array(
        source_population
            .iter()
            .map(NativeMaintenanceRainDevelopmentMember::to_json)
            .collect(),
    );
    Ok(NativeMaintenanceRainDevelopmentPopulation {
        contract_version: NATIVE_MAINTENANCE_RAIN_DEVELOPMENT_POPULATION_VERSION,
        cycle_hours: [0, 6, 12, 18],
        development_end_at,
        development_start_at,
        eligible_row_count,
        excluded_cold_row_count,
        expected_row_count: expected.len(),
        missing_source_row_count,
        missing_target_row_count,
        observed_row_count: source_population.len(),
        operational_horizon_hours: std::array::from_fn(|index| index as u64 + 1),
        population_member_root_sha256: hash_strings(member_hashes)?,
        population_receipt_root_sha256: hash_strings(receipt_hashes)?,
        population_sha256: hash_json(&source_population_json)?,
        source_model_lead_hours: std::array::from_fn(|index| index as u64 + 9),
        source_population,
    })
}

/// Validate one compact source member independently of annual geometry.
fn validate_population_member(
    member: &NativeMaintenanceRainDevelopmentMember,
) -> Result<(), NativeMaintenanceRainMonthlyFitError> {
    let wire = NativeWirePrimitives;
    // retain only bounded value-blind fields and the frozen lead mapping
    if member.key.len() > 128
        || canonical_instant(&wire, &member.issued_at).is_err()
        || canonical_instant(&wire, &member.valid_at).is_err()
        || !valid_hash(&member.source_member_sha256)
        || !valid_hash(&member.source_receipt_sha256)
        || !(9..=31).contains(&member.model_lead_hours)
        || member.operational_horizon_hours != member.model_lead_hours - 8
    {
        return Err(NativeMaintenanceRainMonthlyFitError::Population);
    }
    Ok(())
}

/// Require the exact verified historical projection cross-binding.
fn validate_historical_projection(
    due_month: &str,
    witness: &VerifiedNativeMaintenanceCaptureEpochWitness,
    historical: &NativeMaintenanceHistoricalFitProjection,
) -> Result<(), NativeMaintenanceRainMonthlyFitError> {
    let expected_cutoff = development_interval(due_month)?.1;
    // bind the projection to the same family, month, witness, and immutable cutoff
    if historical.contract_version != NATIVE_MAINTENANCE_HISTORICAL_FIT_VERSION
        || historical.family != NativeMaintenanceHistoricalFamily::Rain
        || historical.due_month != due_month
        || historical.epoch_witness_sha256 != witness.witness_sha256
        || historical.cutoff_at != expected_cutoff
        || !valid_hash(&historical.history_root_sha256)
        || !valid_hash(&historical.member_root_sha256)
        || historical.projection_members.len() > 1_000_000
    {
        return Err(NativeMaintenanceRainMonthlyFitError::HistoricalProjection);
    }
    let mut identities = historical
        .projection_members
        .iter()
        .map(|member| member.member_sha256.clone())
        .chain(
            historical
                .shadow_members
                .iter()
                .map(|member| member.payload_identity_sha256.clone()),
        )
        .collect::<Vec<_>>();
    identities.sort();
    // do not accept a caller-supplied population root without its members
    if hash_strings(identities)? != historical.member_root_sha256 {
        return Err(NativeMaintenanceRainMonthlyFitError::HistoricalProjection);
    }
    Ok(())
}

/// Load the frozen 107 feature names from the retained installed artifact.
fn retained_feature_names() -> Result<JsonValue, NativeMaintenanceRainMonthlyFitError> {
    let artifact = NativeWirePrimitives
        .decode_json(RETAINED_RAIN_ARTIFACT)
        .ok_or(NativeMaintenanceRainMonthlyFitError::Codec)?;
    let names = field(&artifact, "featureNames")
        .cloned()
        .ok_or(NativeMaintenanceRainMonthlyFitError::Codec)?;
    // require the retained artifact's exact frozen feature geometry
    if !matches!(&names, JsonValue::Array(values) if values.len() == 107) {
        return Err(NativeMaintenanceRainMonthlyFitError::Codec);
    }
    Ok(names)
}

/// Retain one consumed member and its receipt identity.
fn retain_consumed(
    member: &NativeMaintenanceHistoricalFitMember,
    members: &mut BTreeSet<String>,
    receipts: &mut BTreeSet<String>,
) {
    members.insert(member.member_sha256.clone());
    receipts.insert(member.receipt.receiptSha256.clone());
}

/// Borrow one decoded document's row array.
fn json_rows(value: &JsonValue) -> Option<&Vec<JsonValue>> {
    let JsonValue::Array(rows) = field(value, "rows")? else {
        return None;
    };
    Some(rows)
}

/// Parse one exact canonical UTC millisecond instant.
fn canonical_instant(
    wire: &NativeWirePrimitives,
    value: &str,
) -> Result<i64, NativeMaintenanceRainMonthlyFitError> {
    let milliseconds = wire
        .parse_explicit_utc_milliseconds(value)
        .ok_or(NativeMaintenanceRainMonthlyFitError::Clock)?;
    // reject normalized aliases at every source clock boundary
    if wire.format_utc_milliseconds(milliseconds).as_deref() != Some(value) {
        return Err(NativeMaintenanceRainMonthlyFitError::Clock);
    }
    Ok(milliseconds)
}

/// Require one consumed clock inside the verified half-open lifecycle interval.
fn causal(value: i64, epoch: i64, cutoff: i64) -> Result<(), NativeMaintenanceRainMonthlyFitError> {
    // reject both pre-epoch history and post-cutoff leakage
    if value < epoch || value >= cutoff {
        return Err(NativeMaintenanceRainMonthlyFitError::Clock);
    }
    Ok(())
}

/// Decode one canonical finite binary64 hexadecimal string.
fn decode_binary64(value: &str) -> Result<f64, NativeMaintenanceRainMonthlyFitError> {
    if value.len() != 16 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(NativeMaintenanceRainMonthlyFitError::Numeric);
    }
    let bits = u64::from_str_radix(value, 16)
        .map_err(|_| NativeMaintenanceRainMonthlyFitError::Numeric)?;
    let value = f64::from_bits(bits);
    // the source decoder admits finite binary64 values only
    if !value.is_finite() {
        return Err(NativeMaintenanceRainMonthlyFitError::Numeric);
    }
    Ok(normalize_zero(value))
}

/// Normalize signed zero exactly where the source creates numerical JSON.
fn normalize_zero(value: f64) -> f64 {
    // JavaScript canonical JSON spells both zero signs as zero
    if value == 0.0 { 0.0 } else { value }
}

/// Require one canonical four-digit due month.
fn valid_month(value: &str) -> bool {
    let bytes = value.as_bytes();
    // retain the predecessor's fixed 20xx month grammar
    bytes.len() == 7
        && bytes.starts_with(b"20")
        && bytes[2].is_ascii_digit()
        && bytes[3].is_ascii_digit()
        && bytes[4] == b'-'
        && matches!(
            &bytes[5..],
            b"01"
                | b"02"
                | b"03"
                | b"04"
                | b"05"
                | b"06"
                | b"07"
                | b"08"
                | b"09"
                | b"10"
                | b"11"
                | b"12"
        )
}

/// Require one positive canonical decimal archive ordinal.
fn valid_positive_ordinal(value: &str) -> bool {
    !value.is_empty()
        && value.as_bytes()[0].is_ascii_digit()
        && value.as_bytes()[0] != b'0'
        && value.bytes().all(|byte| byte.is_ascii_digit())
}

/// Compare arbitrary positive decimal ordinals without numeric conversion.
fn compare_ordinal(left: &str, right: &str) -> Ordering {
    left.len().cmp(&right.len()).then_with(|| left.cmp(right))
}

/// Return one generated receipt projection-kind spelling.
fn receipt_kind(value: AdjustmentRevisionCommitReceiptProjectionKindEnum) -> &'static str {
    match value {
        AdjustmentRevisionCommitReceiptProjectionKindEnum::ActualBestMatch => "actual_best_match",
        AdjustmentRevisionCommitReceiptProjectionKindEnum::NativeSource => "native_source",
        AdjustmentRevisionCommitReceiptProjectionKindEnum::RainGateInput => "rain_gate_input",
        AdjustmentRevisionCommitReceiptProjectionKindEnum::ShadowPrediction => "shadow_prediction",
        AdjustmentRevisionCommitReceiptProjectionKindEnum::TargetRevision => "target_revision",
    }
}

/// Hash one sorted string population as canonical JSON.
fn hash_strings(
    values: impl IntoIterator<Item = String>,
) -> Result<String, NativeMaintenanceRainMonthlyFitError> {
    hash_json(&JsonValue::Array(
        values.into_iter().map(|value| string(&value)).collect(),
    ))
}

/// Hash one recursively sorted canonical JSON value including its terminal line feed.
fn hash_json(value: &JsonValue) -> Result<String, NativeMaintenanceRainMonthlyFitError> {
    let wire = NativeWirePrimitives;
    let bytes = wire
        .encode_sorted_json(value)
        .ok_or(NativeMaintenanceRainMonthlyFitError::Codec)?;
    Ok(digest(&wire, &bytes))
}

/// Hash exact retained bytes with the approved native primitive.
fn digest(wire: &NativeWirePrimitives, bytes: &[u8]) -> String {
    format_digest(RevisionArchiveCanonicalCodec::sha256(wire, bytes))
}

/// Build one JSON object in explicit source field order.
fn object(fields: Vec<(Vec<u16>, JsonValue)>) -> JsonValue {
    JsonValue::Object(fields)
}

/// Build one UTF-16 JSON string.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Build one named UTF-16 JSON string field.
fn string_field(name: &str, value: &str) -> (Vec<u16>, JsonValue) {
    (name.encode_utf16().collect(), string(value))
}

/// Build one named finite JSON number field.
fn number_field(name: &str, value: f64) -> (Vec<u16>, JsonValue) {
    (name.encode_utf16().collect(), JsonValue::Number(value))
}

/// Build one named JSON boolean field.
fn boolean_field(name: &str, value: bool) -> (Vec<u16>, JsonValue) {
    (name.encode_utf16().collect(), JsonValue::Boolean(value))
}

/// Build one named fixed integer-array field.
fn array_number_field<const N: usize>(name: &str, values: &[u64; N]) -> (Vec<u16>, JsonValue) {
    (
        name.encode_utf16().collect(),
        JsonValue::Array(
            values
                .iter()
                .map(|value| JsonValue::Number(*value as f64))
                .collect(),
        ),
    )
}

#[cfg(test)]
#[path = "native_maintenance_rain_monthly_fit_test.rs"]
mod tests;
