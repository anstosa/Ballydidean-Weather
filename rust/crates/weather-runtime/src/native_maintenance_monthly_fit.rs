//! Exact temperature and wind monthly fit-input assembly over verified history.

use std::cmp::Ordering;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt::{self, Display, Formatter};

use jiff::{Span, civil::Date};
use weather_contracts::{
    ForecastAdjustmentMetric, ForecastObservationProviderFamily, ForecastObservationStationKey,
    JsonValue, retained_forecast_observation,
};
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};

use crate::maintenance_canonical_json::{field, text, valid_hash};
use crate::native_fit_ascii_collation::native_fit_collation_cmp;
use crate::native_json::NativeJson;
use crate::native_maintenance_historical_catalog::NativeMaintenanceHistoricalFamily;
use crate::native_maintenance_historical_fit::{
    NATIVE_MAINTENANCE_HISTORICAL_FIT_VERSION, NativeMaintenanceHistoricalFitMember,
    NativeMaintenanceHistoricalFitProjection,
};
use crate::native_maintenance_retirement_files_v2::format_digest;
use crate::{
    NativePredecessorModelCalendar, NativeWirePrimitives,
    VerifiedNativeMaintenanceCaptureEpochWitness,
};

pub const NATIVE_MAINTENANCE_MONTHLY_FIT_VERSION: &str = "adjustment-monthly-fit-input-assembly/v1";

const TEMPERATURE_FIT_VERSION: &str = "temperature-maintenance-fit-input/v2";
const WIND_FIT_VERSION: &str = "wind-maintenance-fit-input/v2";
const TEMPERATURE_NATIVE_VERSION: &str = "adjustment-temperature-native-source-projection/v2";
const DEVELOPMENT_DATES: usize = 90;
const WIND_EPOCH_DATES: usize = 402;
const HOUR_MILLISECONDS: i64 = 3_600_000;

/// Verified installed temperature incumbent retained by the fit input.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceTemperatureIncumbent {
    pub artifact_bytes: Vec<u8>,
    pub artifact_identity_sha256: String,
    pub authority_kind: String,
    pub family: NativeMaintenanceHistoricalFamily,
    pub model: JsonValue,
    pub state: String,
}

/// Genuine immutable-cutoff reason for completing without a candidate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceMonthlyNoCandidateReason {
    IncompleteDevelopmentPopulation,
}

impl NativeMaintenanceMonthlyNoCandidateReason {
    /// Return the exact semantic-inspection reason spelling.
    pub const fn as_str(self) -> &'static str {
        "incomplete_development_population"
    }
}

/// Exact closed fit-input assembly and its canonical private identity.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceMonthlyFitAssembly {
    pub contract_version: &'static str,
    pub cutoff_at: String,
    pub due_month: String,
    pub epoch_witness_sha256: String,
    pub family: NativeMaintenanceHistoricalFamily,
    pub fit_input: JsonValue,
    pub historical_member_root_sha256: String,
    pub input_manifest_sha256: String,
}

/// Closed semantic result consumed by production inspection.
#[derive(Clone, Debug, PartialEq)]
pub enum NativeMaintenanceMonthlyFitOutcome {
    NoCandidate {
        historical_member_root_sha256: String,
        reason: NativeMaintenanceMonthlyNoCandidateReason,
    },
    Ready(NativeMaintenanceMonthlyFitAssembly),
}

/// Closed monthly projection admission and join failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceMonthlyFitError {
    Input,
    Incumbent,
    HistoricalProjection,
    NativeProjection,
    RecentErrorState,
    BestMatch,
    Target,
    Receipt,
    WindEpoch,
    Lineage,
    Calendar,
    Numeric,
    Codec,
}

impl Display for NativeMaintenanceMonthlyFitError {
    /// Render one bounded diagnostic without value-bearing fit material.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Input => "monthly fit input is invalid",
            Self::Incumbent => "monthly fit incumbent is unavailable",
            Self::HistoricalProjection => "monthly historical projection differs",
            Self::NativeProjection => "temperature monthly native projection is unavailable",
            Self::RecentErrorState => "temperature monthly recent-error state differs",
            Self::BestMatch => "temperature monthly best-match projection differs",
            Self::Target => "monthly physical target projection differs",
            Self::Receipt => "monthly fit receipt exceeds its causal cutoff",
            Self::WindEpoch => "wind monthly archive epoch is incomplete",
            Self::Lineage => "monthly physical source lineage differs",
            Self::Calendar => "monthly fit calendar is invalid",
            Self::Numeric => "monthly fit numeric value is invalid",
            Self::Codec => "monthly fit canonical encoding failed",
        })
    }
}

impl std::error::Error for NativeMaintenanceMonthlyFitError {}

/// Assemble one source-exact temperature or wind fit input.
pub fn build_native_adjustment_monthly_fit_assembly(
    due_month: &str,
    epoch_witness: &VerifiedNativeMaintenanceCaptureEpochWitness,
    family: NativeMaintenanceHistoricalFamily,
    historical: &NativeMaintenanceHistoricalFitProjection,
    incumbent: Option<&NativeMaintenanceTemperatureIncumbent>,
) -> Result<NativeMaintenanceMonthlyFitOutcome, NativeMaintenanceMonthlyFitError> {
    use NativeMaintenanceMonthlyFitError as Error;

    // refuse rain at the disjoint temperature and wind fitter boundary
    if !matches!(
        family,
        NativeMaintenanceHistoricalFamily::Temperature | NativeMaintenanceHistoricalFamily::Wind
    ) || !valid_month(due_month)
        || !valid_hash(&epoch_witness.witness_sha256)
    {
        return Err(Error::Input);
    }
    validate_historical_projection(due_month, &epoch_witness.witness_sha256, family, historical)?;
    let available: HashSet<&str> = historical
        .projection_members
        .iter()
        .map(|member| member.projection_kind.as_str())
        .collect();
    let required = match family {
        NativeMaintenanceHistoricalFamily::Temperature => {
            &["actual_best_match", "native_source", "target_revision"][..]
        }
        NativeMaintenanceHistoricalFamily::Wind => &["actual_best_match", "target_revision"][..],
        NativeMaintenanceHistoricalFamily::Rain => return Err(Error::Input),
    };
    // complete an empty original cutoff rather than starving later months
    if required.iter().any(|kind| !available.contains(kind)) {
        return Ok(NativeMaintenanceMonthlyFitOutcome::NoCandidate {
            historical_member_root_sha256: historical.member_root_sha256.clone(),
            reason: NativeMaintenanceMonthlyNoCandidateReason::IncompleteDevelopmentPopulation,
        });
    }
    let fit_input = match family {
        NativeMaintenanceHistoricalFamily::Temperature => {
            build_temperature_fit_input(historical, incumbent.ok_or(Error::Incumbent)?, due_month)?
        }
        NativeMaintenanceHistoricalFamily::Wind => {
            if incumbent.is_some() {
                return Err(Error::Incumbent);
            }
            build_wind_fit_input(historical, due_month, &epoch_witness.witness_sha256)?
        }
        NativeMaintenanceHistoricalFamily::Rain => return Err(Error::Input),
    };
    let input_manifest_sha256 = hash_json(&fit_input)?;
    let assembly = NativeMaintenanceMonthlyFitAssembly {
        contract_version: NATIVE_MAINTENANCE_MONTHLY_FIT_VERSION,
        cutoff_at: historical.cutoff_at.clone(),
        due_month: due_month.to_owned(),
        epoch_witness_sha256: epoch_witness.witness_sha256.clone(),
        family,
        fit_input,
        historical_member_root_sha256: historical.member_root_sha256.clone(),
        input_manifest_sha256,
    };
    validate_native_adjustment_monthly_fit_assembly(&assembly)?;
    Ok(NativeMaintenanceMonthlyFitOutcome::Ready(assembly))
}

/// Revalidate one closed assembly and its exact private fit-input identity.
pub fn validate_native_adjustment_monthly_fit_assembly(
    value: &NativeMaintenanceMonthlyFitAssembly,
) -> Result<(), NativeMaintenanceMonthlyFitError> {
    use NativeMaintenanceMonthlyFitError::Input;

    let fit_version = text(&value.fit_input, "contractVersion").ok_or(Input)?;
    // bind only the two reviewed fit contracts and every outer identity
    if value.contract_version != NATIVE_MAINTENANCE_MONTHLY_FIT_VERSION
        || !valid_month(&value.due_month)
        || !valid_instant(&value.cutoff_at)
        || !valid_hash(&value.epoch_witness_sha256)
        || !valid_hash(&value.historical_member_root_sha256)
        || !valid_hash(&value.input_manifest_sha256)
        || fit_version
            != match value.family {
                NativeMaintenanceHistoricalFamily::Temperature => TEMPERATURE_FIT_VERSION,
                NativeMaintenanceHistoricalFamily::Wind => WIND_FIT_VERSION,
                NativeMaintenanceHistoricalFamily::Rain => return Err(Input),
            }
        || hash_json(&value.fit_input)? != value.input_manifest_sha256
    {
        return Err(Input);
    }
    Ok(())
}

/// Require the exact verified-history cross-binding consumed by this assembler.
fn validate_historical_projection(
    due_month: &str,
    witness_sha256: &str,
    family: NativeMaintenanceHistoricalFamily,
    historical: &NativeMaintenanceHistoricalFitProjection,
) -> Result<(), NativeMaintenanceMonthlyFitError> {
    // bind the immutable projection to the requested family, month, and witness
    if historical.contract_version != NATIVE_MAINTENANCE_HISTORICAL_FIT_VERSION
        || historical.family != family
        || historical.due_month != due_month
        || historical.epoch_witness_sha256 != witness_sha256
        || !valid_hash(&historical.history_root_sha256)
        || !valid_hash(&historical.member_root_sha256)
        || !valid_instant(&historical.cutoff_at)
    {
        return Err(NativeMaintenanceMonthlyFitError::HistoricalProjection);
    }
    Ok(())
}

#[derive(Clone)]
struct TemperatureTargetRow {
    archive_commit_ordinal: String,
    archive_committed_at: String,
    logical_received_at: String,
    physical_station_key: ForecastObservationStationKey,
    source_id: String,
    temperature_c64: Option<String>,
    valid_at: String,
}

struct TemperatureNetworkTarget {
    actual_temperature_c: f64,
    maximum_target_receipt_at: String,
}

/// Build exact temperature rows from native source, best match, and targets.
fn build_temperature_fit_input(
    projection: &NativeMaintenanceHistoricalFitProjection,
    incumbent: &NativeMaintenanceTemperatureIncumbent,
    due_month: &str,
) -> Result<JsonValue, NativeMaintenanceMonthlyFitError> {
    use NativeMaintenanceMonthlyFitError as Error;

    // require the actual authenticated installed temperature model owner
    if incumbent.family != NativeMaintenanceHistoricalFamily::Temperature
        || incumbent.artifact_bytes.is_empty()
        || !valid_hash(&incumbent.artifact_identity_sha256)
        || !matches!(incumbent.model, JsonValue::Object(_))
    {
        return Err(Error::Incumbent);
    }
    let target_members: Vec<_> = projection
        .projection_members
        .iter()
        .filter(|member| member.projection_kind == "target_revision")
        .collect();
    let target_rows = temperature_target_rows(&target_members)?;
    let native_members: Vec<_> = projection
        .projection_members
        .iter()
        .filter(|member| member.projection_kind == "native_source")
        .collect();
    let best_match = temperature_best_match_members(&projection.projection_members)?;
    let mut native_documents: Vec<(&str, &JsonValue, Vec<&NativeMaintenanceHistoricalFitMember>)> =
        Vec::new();
    // retain each complete native body once in first-member order
    for &member in &native_members {
        if text(&member.document, "contractVersion").as_deref() != Some(TEMPERATURE_NATIVE_VERSION)
            || text(&member.document, "family").as_deref() != Some("temperature")
        {
            return Err(Error::NativeProjection);
        }
        // reject one payload identity resolving to different parsed documents
        if let Some((_, document, members)) = native_documents
            .iter_mut()
            .find(|(identity, _, _)| *identity == member.payload_identity_sha256.as_str())
        {
            if *document != &member.document {
                return Err(Error::NativeProjection);
            }
            members.push(member);
        } else {
            native_documents.push((
                &member.payload_identity_sha256,
                &member.document,
                vec![member],
            ));
        }
    }
    let mut rows = Vec::new();
    // replay every frozen source decision before consuming selected rows
    for (_, source, selected_members) in native_documents {
        let run_initialized_at = nested_text(source, "logicalKey", "runInitializedAt")?;
        let replayed_state = replay_temperature_recent_error_state(
            &native_members,
            &target_rows,
            &run_initialized_at,
        )?;
        let stored_state = field(source, "recentErrorState").ok_or(Error::RecentErrorState)?;
        let stored_state_sha256 =
            text(source, "recentErrorStateSha256").ok_or(Error::RecentErrorState)?;
        // bind both state value and producer-ordered database hash
        if hash_json(stored_state)? != hash_json(&replayed_state)?
            || ordered_json_sha256(&replayed_state)? != stored_state_sha256
        {
            return Err(Error::RecentErrorState);
        }
        for native in selected_members {
            let lead = unsigned(&native.row, "modelLeadHours")?;
            // omit the non-serving warmup horizon without inventing comparators
            if lead < 7 {
                continue;
            }
            let best_content = optional_text(&native.row, "bestMatchContentSha256")?;
            let best_run = optional_text(&native.row, "bestMatchProductRunAt")?;
            let best_source = optional_text(&native.row, "bestMatchSourceId")?;
            let best_temperature = optional_text(&native.row, "bestMatchTemperatureC64")?;
            let valid_at = text(&native.row, "validAt").ok_or(Error::BestMatch)?;
            let key = temperature_best_match_key(
                best_source.as_deref(),
                best_run.as_deref(),
                &valid_at,
                best_content.as_deref(),
            );
            let comparator = best_match.get(&key).ok_or(Error::BestMatch)?;
            let comparator_temperature = optional_text(&comparator.row, "temperatureC64")?;
            let target =
                temperature_network_target_at(&target_rows, &valid_at, &projection.cutoff_at)?
                    .ok_or(Error::Target)?;
            // prohibit a native row lacking any authenticated causal class
            if best_content.is_none()
                || best_run.is_none()
                || best_source.is_none()
                || best_temperature.is_none()
                || comparator_temperature != best_temperature
            {
                return Err(Error::BestMatch);
            }
            let source_received = text(source, "logicalReceivedAt").ok_or(Error::BestMatch)?;
            let comparator_received =
                text(&comparator.document, "logicalReceivedAt").ok_or(Error::BestMatch)?;
            // require the exact comparator by the frozen native decision
            if comparator_received.as_str() > source_received.as_str()
                || comparator.receipt.archiveCommittedAt.as_str() > source_received.as_str()
            {
                return Err(Error::BestMatch);
            }
            let source_receipt_at = std::cmp::max(
                native.receipt.archiveCommittedAt.as_str(),
                comparator.receipt.archiveCommittedAt.as_str(),
            );
            // keep all receipts strictly before the immutable cutoff
            if source_receipt_at >= projection.cutoff_at.as_str()
                || target.maximum_target_receipt_at.as_str() >= projection.cutoff_at.as_str()
            {
                return Err(Error::Receipt);
            }
            let source_object = field(source, "source").ok_or(Error::NativeProjection)?;
            rows.push(object(vec![
                number_field("actualTemperatureC", target.actual_temperature_c),
                string_field(
                    "adapterVersion",
                    &text(source_object, "adapterVersion").ok_or(Error::NativeProjection)?,
                ),
                string_field("cohort", "ecmwf_single_run_hindcast"),
                string_field("evidenceClass", "development"),
                string_field("firstReceivedAt", &source_received),
                string_field("key", &native.member_sha256),
                string_field(
                    "modelCycle",
                    &text(&native.row, "modelCycle").ok_or(Error::NativeProjection)?,
                ),
                number_field("modelLeadHours", lead as f64),
                number_field("operationalHorizonHours", (lead - 6) as f64),
                string_field(
                    "providerResponseSha256",
                    &nested_text(source, "logicalKey", "providerResponseSha256")?,
                ),
                number_field(
                    "rawBestMatchTemperatureC",
                    decode_binary64(comparator_temperature.as_deref().ok_or(Error::BestMatch)?)?,
                ),
                optional_number_field(
                    "rawRelativeHumidityPercent",
                    optional_binary64(&native.row, "rawRelativeHumidityPercent64")?,
                ),
                number_field(
                    "rawTemperatureC",
                    decode_binary64(
                        &text(&native.row, "rawTemperatureC64").ok_or(Error::NativeProjection)?,
                    )?,
                ),
                optional_number_field(
                    "rawWindSpeedMps",
                    optional_binary64(&native.row, "rawWindSpeedMps64")?,
                ),
                string_field("runInitializedAt", &run_initialized_at),
                string_field("sourceReceiptAt", source_receipt_at),
                ("state".encode_utf16().collect(), replayed_state.clone()),
                string_field("targetMaxReceiptAt", &target.maximum_target_receipt_at),
                string_field("validAt", &valid_at),
            ]));
        }
    }
    rows.sort_by(|left, right| {
        text(left, "validAt")
            .cmp(&text(right, "validAt"))
            .then_with(|| text(left, "key").cmp(&text(right, "key")))
    });
    let calendar = NativePredecessorModelCalendar::new().ok_or(Error::Calendar)?;
    let mut dates = BTreeSet::new();
    // collect actual LA dates from the admitted predecessor calendar
    for row in &rows {
        dates.insert(local_date(
            &calendar,
            &text(row, "validAt").ok_or(Error::Calendar)?,
        )?);
    }
    let development_dates: HashSet<String> =
        dates.into_iter().rev().take(DEVELOPMENT_DATES).collect();
    let mut development_rows = Vec::new();
    let mut training_rows = Vec::new();
    // partition without changing the already-sorted row order
    for row in rows {
        let date = local_date(&calendar, &text(&row, "validAt").ok_or(Error::Calendar)?)?;
        if development_dates.contains(&date) {
            development_rows.push(row);
        } else {
            training_rows.push(row);
        }
    }
    Ok(object(vec![
        string_field("contractVersion", TEMPERATURE_FIT_VERSION),
        (
            "developmentRows".encode_utf16().collect(),
            JsonValue::Array(development_rows),
        ),
        (
            "incumbentModel".encode_utf16().collect(),
            incumbent.model.clone(),
        ),
        string_field("month", due_month),
        (
            "trainingRows".encode_utf16().collect(),
            JsonValue::Array(training_rows),
        ),
    ]))
}

/// One independently projected temperature target cell and optional native value.
type ProjectedTemperatureTarget = (
    String,
    ForecastObservationStationKey,
    String,
    Option<String>,
);

/// Authenticate target bodies and bind every row to its database receipt.
fn temperature_target_rows(
    members: &[&NativeMaintenanceHistoricalFitMember],
) -> Result<Vec<TemperatureTargetRow>, NativeMaintenanceMonthlyFitError> {
    use NativeMaintenanceMonthlyFitError as Error;

    let mut projected_by_body: HashMap<&str, Vec<ProjectedTemperatureTarget>> = HashMap::new();
    // independently project each unique target body through the frozen lineage table
    for member in members {
        // project a retained body once rather than once for each of its cells
        if let std::collections::hash_map::Entry::Vacant(entry) =
            projected_by_body.entry(&member.payload_identity_sha256)
        {
            entry.insert(project_temperature_target_body(member)?);
        }
    }
    members
        .iter()
        .map(|member| {
            let content = text(&member.row, "contentSha256").ok_or(Error::Target)?;
            let valid_at = text(&member.row, "validAt").ok_or(Error::Target)?;
            let matches: Vec<_> = projected_by_body
                .get(member.payload_identity_sha256.as_str())
                .ok_or(Error::Target)?
                .iter()
                .filter(|(candidate_content, _, candidate_valid, _)| {
                    candidate_content == &content && candidate_valid == &valid_at
                })
                .collect();
            if matches.len() != 1 {
                return Err(Error::Target);
            }
            let (_, station, _, temperature_c64) = matches[0];
            Ok(TemperatureTargetRow {
                archive_commit_ordinal: member.receipt.archiveCommitOrdinal.clone(),
                archive_committed_at: member.receipt.archiveCommittedAt.clone(),
                logical_received_at: text(&member.document, "logicalReceivedAt")
                    .ok_or(Error::Target)?,
                physical_station_key: *station,
                source_id: nested_text(&member.document, "logicalKey", "sourceId")?,
                temperature_c64: temperature_c64.clone(),
                valid_at,
            })
        })
        .collect()
}

/// Project one canonical physical source body through the retained lineage roster.
fn project_temperature_target_body(
    member: &NativeMaintenanceHistoricalFitMember,
) -> Result<Vec<ProjectedTemperatureTarget>, NativeMaintenanceMonthlyFitError> {
    use NativeMaintenanceMonthlyFitError as Error;

    let source = field(&member.document, "source").ok_or(Error::Lineage)?;
    let source_key = text(source, "sourceKey").ok_or(Error::Lineage)?;
    let lineage = source_lineage(&source_key).ok_or(Error::Lineage)?;
    if text(source, "adapterVersion").as_deref() != Some(lineage.adapter_contract)
        || text(source, "sourceConfigFingerprint").as_deref() != Some(lineage.fingerprint)
    {
        return Err(Error::Lineage);
    }
    let JsonValue::Array(rows) = field(&member.document, "rows").ok_or(Error::Target)? else {
        return Err(Error::Target);
    };
    let mut projected = Vec::with_capacity(rows.len());
    let mut clocks = HashSet::new();
    // retain every normalized row including explicit missing temperature
    for row in rows {
        let valid_at = text(row, "validAt").ok_or(Error::Target)?;
        admit_lineage_clock(&valid_at, lineage)?;
        if !clocks.insert(valid_at.clone()) {
            return Err(Error::Target);
        }
        projected.push((
            text(row, "contentSha256").ok_or(Error::Target)?,
            lineage.station,
            valid_at,
            optional_text(row, "temperatureC64")?,
        ));
    }
    Ok(projected)
}

/// Index full best-match tuples and reject duplicate receipt authority.
fn temperature_best_match_members(
    members: &[NativeMaintenanceHistoricalFitMember],
) -> Result<HashMap<String, &NativeMaintenanceHistoricalFitMember>, NativeMaintenanceMonthlyFitError>
{
    let mut by_tuple = HashMap::new();
    // admit only authenticated actual best-match projection members
    for member in members
        .iter()
        .filter(|member| member.projection_kind == "actual_best_match")
    {
        let key = temperature_best_match_key(
            nested_optional_text(&member.document, "logicalKey", "sourceId")?.as_deref(),
            nested_optional_text(&member.document, "logicalKey", "productRunAt")?.as_deref(),
            &text(&member.row, "validAt").ok_or(NativeMaintenanceMonthlyFitError::BestMatch)?,
            optional_text(&member.row, "contentSha256")?.as_deref(),
        );
        if by_tuple.insert(key, member).is_some() {
            return Err(NativeMaintenanceMonthlyFitError::BestMatch);
        }
    }
    Ok(by_tuple)
}

/// Encode one exact best-match source and content tuple.
fn temperature_best_match_key(
    source_id: Option<&str>,
    product_run_at: Option<&str>,
    valid_at: &str,
    content_sha256: Option<&str>,
) -> String {
    [
        source_id.unwrap_or(""),
        product_run_at.unwrap_or(""),
        valid_at,
        content_sha256.unwrap_or(""),
    ]
    .join("\n")
}

/// Replay the exact causal seventy-two-hour recent-error recipe.
fn replay_temperature_recent_error_state(
    native_members: &[&NativeMaintenanceHistoricalFitMember],
    target_rows: &[TemperatureTargetRow],
    target_run_initialized_at: &str,
) -> Result<JsonValue, NativeMaintenanceMonthlyFitError> {
    use NativeMaintenanceMonthlyFitError as Error;

    let wire = NativeWirePrimitives;
    let target_ms = instant(&wire, target_run_initialized_at)?;
    let window_end_ms = target_ms
        .checked_sub(7 * HOUR_MILLISECONDS)
        .ok_or(Error::Calendar)?;
    let window_end = wire
        .format_utc_milliseconds(window_end_ms)
        .ok_or(Error::Calendar)?;
    let window_start = window_end_ms
        .checked_sub(71 * HOUR_MILLISECONDS)
        .ok_or(Error::Calendar)?;
    let mut prior_by_valid: HashMap<String, (&NativeMaintenanceHistoricalFitMember, String)> =
        HashMap::new();
    // select the latest genuinely available lead-seven-to-eighteen source per hour
    for member in native_members {
        let run = nested_text(&member.document, "logicalKey", "runInitializedAt")?;
        let valid = text(&member.row, "validAt").ok_or(Error::RecentErrorState)?;
        let valid_ms = instant(&wire, &valid)?;
        let lead = unsigned(&member.row, "modelLeadHours")?;
        if text(&member.document, "contractVersion").as_deref() != Some(TEMPERATURE_NATIVE_VERSION)
            || !(7..=18).contains(&lead)
            || run.as_str() >= target_run_initialized_at
            || text(&member.document, "logicalReceivedAt")
                .ok_or(Error::RecentErrorState)?
                .as_str()
                > target_run_initialized_at
            || member.receipt.archiveCommittedAt.as_str() > target_run_initialized_at
            || valid_ms < window_start
            || valid_ms > window_end_ms
        {
            continue;
        }
        let replace = prior_by_valid
            .get(&valid)
            .is_none_or(|(current, current_run)| {
                run.as_str() > current_run.as_str()
                    || run.as_str() == current_run.as_str()
                        && compare_ordinal(
                            &member.receipt.archiveCommitOrdinal,
                            &current.receipt.archiveCommitOrdinal,
                        ) == Ordering::Greater
            });
        if replace {
            prior_by_valid.insert(valid, (member, run));
        }
    }
    struct Selected {
        error_c: f64,
        key: String,
        run: String,
        valid: String,
    }
    let mut selected = Vec::new();
    let mut valid_ms = window_start;
    // replay all exact inclusive state hours in chronological order
    while valid_ms <= window_end_ms {
        let valid = wire
            .format_utc_milliseconds(valid_ms)
            .ok_or(Error::Calendar)?;
        if let Some((prior, run)) = prior_by_valid.get(&valid)
            && let Some(target) =
                temperature_network_target_at(target_rows, &valid, target_run_initialized_at)?
        {
            selected.push(Selected {
                error_c: target.actual_temperature_c
                    - decode_binary64(
                        &text(&prior.row, "rawTemperatureC64").ok_or(Error::RecentErrorState)?,
                    )?,
                key: format!("{run}/{valid}"),
                run: run.clone(),
                valid: valid.clone(),
            });
        }
        valid_ms = valid_ms
            .checked_add(HOUR_MILLISECONDS)
            .ok_or(Error::Calendar)?;
    }
    let short_start = window_end_ms - 23 * HOUR_MILLISECONDS;
    let short: Vec<_> = selected
        .iter()
        .filter(|item| instant(&wire, &item.valid).is_ok_and(|value| value >= short_start))
        .collect();
    let calendar = NativePredecessorModelCalendar::new().ok_or(Error::Calendar)?;
    let local_dates: HashSet<_> = selected
        .iter()
        .map(|item| local_date(&calendar, &item.valid))
        .collect::<Result<_, _>>()?;
    let short_supported = short.len() >= 6;
    let long_supported = selected.len() >= 24;
    let raw_b72 = long_supported
        .then(|| median(selected.iter().map(|item| item.error_c).collect()))
        .transpose()?;
    let b24 = short_supported
        .then(|| median(short.iter().map(|item| item.error_c).collect()))
        .transpose()?
        .map(clip_temperature_state);
    let b72 = raw_b72.map(clip_temperature_state);
    let mad72 = raw_b72
        .map(|center| {
            median(
                selected
                    .iter()
                    .map(|item| (item.error_c - center).abs())
                    .collect(),
            )
            .map(|value| value.min(6.0))
        })
        .transpose()?;
    let maximum_run = selected.iter().map(|item| item.run.as_str()).max();
    Ok(object(vec![
        optional_number_field("b24C", b24),
        optional_number_field("b72C", b72),
        string_field("cohort", "ecmwf_single_run_hindcast"),
        number_field("localDates", local_dates.len() as f64),
        optional_number_field("mad72C", mad72),
        optional_string_field("maximumSourceRunInitializedAt", maximum_run),
        optional_string_field(
            "maximumSourceValidAt",
            selected.last().map(|item| item.valid.as_str()),
        ),
        number_field("n24", short.len() as f64),
        number_field("n72", selected.len() as f64),
        (
            "sourceKeys".encode_utf16().collect(),
            JsonValue::Array(selected.iter().map(|item| string(&item.key)).collect()),
        ),
        (
            "supported".encode_utf16().collect(),
            JsonValue::Boolean(short_supported && long_supported && local_dates.len() >= 2),
        ),
        string_field("targetRunInitializedAt", target_run_initialized_at),
        string_field("windowEndValidAt", &window_end),
    ]))
}

/// Select one deterministic network target available at the supplied causal clock.
fn temperature_network_target_at(
    rows: &[TemperatureTargetRow],
    valid_at: &str,
    as_of: &str,
) -> Result<Option<TemperatureNetworkTarget>, NativeMaintenanceMonthlyFitError> {
    use NativeMaintenanceMonthlyFitError as Error;

    let mut current: HashMap<(String, String), &TemperatureTargetRow> = HashMap::new();
    // retain the last archived revision for each physical source clock
    for row in rows {
        if row.logical_received_at.as_str() > as_of || row.archive_committed_at.as_str() > as_of {
            continue;
        }
        let key = (row.source_id.clone(), row.valid_at.clone());
        let replace = current.get(&key).is_none_or(|existing| {
            compare_ordinal(
                &row.archive_commit_ordinal,
                &existing.archive_commit_ordinal,
            ) == Ordering::Greater
        });
        if replace {
            current.insert(key, row);
        }
    }
    let wire = NativeWirePrimitives;
    let target_ms = instant(&wire, valid_at)?;
    let mut selected = Vec::new();
    // choose the closest earlier-tied row for every frozen station
    for station in retained_forecast_observation::STATIONS {
        let mut candidates: Vec<_> = current
            .values()
            .copied()
            .filter(|row| {
                row.physical_station_key == station.key
                    && row.temperature_c64.is_some()
                    && instant(&wire, &row.valid_at).is_ok_and(|clock| {
                        clock >= target_ms - 5 * 60_000 && clock < target_ms + 5 * 60_000
                    })
            })
            .collect();
        candidates.sort_by(|left, right| {
            let left_ms = instant(&wire, &left.valid_at).expect("filtered target clock");
            let right_ms = instant(&wire, &right.valid_at).expect("filtered target clock");
            left_ms
                .abs_diff(target_ms)
                .cmp(&right_ms.abs_diff(target_ms))
                .then_with(|| left.valid_at.cmp(&right.valid_at))
                .then_with(|| compare_ordinal(&left.source_id, &right.source_id))
        });
        if let Some(row) = candidates.first() {
            selected.push(*row);
        }
    }
    let actual = scalar_network_actual(&selected)?;
    let Some(actual_temperature_c) = actual else {
        return Ok(None);
    };
    let maximum_target_receipt_at = selected
        .iter()
        .map(|row| row.archive_committed_at.as_str())
        .max()
        .ok_or(Error::Target)?
        .to_owned();
    Ok(Some(TemperatureNetworkTarget {
        actual_temperature_c,
        maximum_target_receipt_at,
    }))
}

/// Apply the exact normalized strict-half station-weighted scalar.
fn scalar_network_actual(
    selected: &[&TemperatureTargetRow],
) -> Result<Option<f64>, NativeMaintenanceMonthlyFitError> {
    let mut values = Vec::new();
    // join every selected station to its exact frozen geometry
    for row in selected {
        let station = retained_forecast_observation::STATIONS
            .iter()
            .find(|station| station.key == row.physical_station_key)
            .ok_or(NativeMaintenanceMonthlyFitError::Target)?;
        values.push((
            station.key,
            station.nearest_rank,
            station.unnormalized_spatial_weight,
            decode_binary64(
                row.temperature_c64
                    .as_deref()
                    .ok_or(NativeMaintenanceMonthlyFitError::Target)?,
            )?,
        ));
    }
    // preserve insufficient physical coverage without imputation
    if values.len() < 3 || !values.iter().any(|(_, rank, _, _)| *rank <= 3) {
        return Ok(None);
    }
    values
        .sort_by(|left, right| native_fit_collation_cmp(station_key(left.0), station_key(right.0)));
    let total: f64 = values.iter().map(|(_, _, weight, _)| *weight).sum();
    if !total.is_finite() || total <= 0.0 {
        return Err(NativeMaintenanceMonthlyFitError::Numeric);
    }
    let mut weighted: Vec<_> = values
        .into_iter()
        .map(|(key, _, weight, value)| (value, station_key(key), weight / total))
        .collect();
    weighted.sort_by(|left, right| {
        left.0
            .partial_cmp(&right.0)
            .unwrap_or(Ordering::Equal)
            .then_with(|| left.1.cmp(right.1))
    });
    let normalized_total: f64 = weighted.iter().map(|item| item.2).sum();
    let mut distinct: Vec<(f64, f64)> = Vec::new();
    // combine exact equal station values after weight normalization
    for (value, _, weight) in weighted {
        if let Some(last) = distinct.last_mut()
            && last.0 == value
        {
            last.1 += weight;
        } else {
            distinct.push((value, weight));
        }
    }
    let half = normalized_total / 2.0;
    let mut cumulative = 0.0;
    // select the first strict half crossing or exact midpoint tie
    for (index, item) in distinct.iter().enumerate() {
        cumulative += item.1;
        if cumulative > half {
            return Ok(Some(item.0));
        }
        if cumulative == half {
            return Ok(Some(
                distinct
                    .get(index + 1)
                    .map_or(item.0, |next| (item.0 + next.0) / 2.0),
            ));
        }
    }
    Ok(distinct.last().map(|item| item.0))
}

/// Build one disjoint archive-derived wind manifest and sanitized row population.
fn build_wind_fit_input(
    projection: &NativeMaintenanceHistoricalFitProjection,
    due_month: &str,
    witness_sha256: &str,
) -> Result<JsonValue, NativeMaintenanceMonthlyFitError> {
    use NativeMaintenanceMonthlyFitError as Error;

    let calendar = NativePredecessorModelCalendar::new().ok_or(Error::Calendar)?;
    let mut target_receipt_by_valid: HashMap<String, String> = HashMap::new();
    // derive the latest target receipt at every physical target clock
    for member in projection
        .projection_members
        .iter()
        .filter(|member| member.projection_kind == "target_revision")
    {
        let valid = text(&member.row, "validAt").ok_or(Error::Target)?;
        let receipt = &member.receipt.archiveCommittedAt;
        target_receipt_by_valid
            .entry(valid)
            .and_modify(|current| {
                if current.as_str() < receipt.as_str() {
                    *current = receipt.clone();
                }
            })
            .or_insert_with(|| receipt.clone());
    }
    let mut rows = Vec::new();
    let mut members = Vec::new();
    let mut opened = Vec::new();
    // project only database-backed forecast and physical-target rows
    for member in &projection.projection_members {
        if !matches!(
            member.projection_kind.as_str(),
            "actual_best_match" | "target_revision"
        ) {
            continue;
        }
        let valid_at = text(&member.row, "validAt").ok_or(Error::Target)?;
        let date = local_date(&calendar, &valid_at)?;
        let target_receipt_at = target_receipt_by_valid
            .get(&valid_at)
            .ok_or(Error::Target)?;
        let sanitized = if member.projection_kind == "actual_best_match" {
            wind_forecast_row(member)?
        } else {
            wind_target_row(member)?
        };
        let kind = if member.projection_kind == "actual_best_match" {
            "forecast"
        } else {
            "target"
        };
        let station_key = if text(&sanitized, "recordKind").as_deref() == Some("station_hour") {
            text(&sanitized, "physicalStationKey")
                .as_deref()
                .map(string)
                .unwrap_or(JsonValue::Null)
        } else {
            JsonValue::Null
        };
        members.push(object(vec![
            string_field("localDate", &date),
            string_field("maxValidAt", &valid_at),
            string_field("minValidAt", &valid_at),
            string_field(
                "path",
                &format!("archive-members/{date}/{kind}/{}", member.member_sha256),
            ),
            number_field("plaintextBytes", member.payload_bytes.len() as f64),
            string_field("recordKind", &member.projection_kind),
            number_field("rowCount", 1.0),
            string_field("sha256", &member.member_sha256),
            number_field("sizeBytes", member.payload_bytes.len() as f64),
            ("stationKey".encode_utf16().collect(), station_key),
        ]));
        opened.push(object(vec![
            string_field("maximumSourceReceiptAt", &member.receipt.archiveCommittedAt),
            string_field("maximumTargetReceiptAt", target_receipt_at),
            string_field("memberSha256", &member.member_sha256),
        ]));
        rows.push(sanitized);
    }
    members.sort_by(|left, right| {
        let left = text(left, "path").expect("constructed wind member path");
        let right = text(right, "path").expect("constructed wind member path");
        native_fit_collation_cmp(&left, &right)
    });
    opened.sort_by_key(|member| text(member, "memberSha256"));
    let cutoff_ms = instant(&NativeWirePrimitives, &projection.cutoff_at)?;
    let cutoff_minus_one = NativeWirePrimitives
        .format_utc_milliseconds(cutoff_ms - 1)
        .ok_or(Error::Calendar)?;
    let cutoff_date = local_date(&calendar, &cutoff_minus_one)?;
    let selected_dates = trailing_local_dates(&cutoff_date, WIND_EPOCH_DATES)?;
    let selected: HashSet<&str> = selected_dates.iter().map(String::as_str).collect();
    let selected_members: Vec<_> = members
        .into_iter()
        .filter(|member| {
            text(member, "localDate")
                .as_deref()
                .is_some_and(|date| selected.contains(date))
        })
        .collect();
    let identities: HashSet<String> = selected_members
        .iter()
        .filter_map(|member| text(member, "sha256"))
        .collect();
    let selected_rows: Vec<_> = rows
        .into_iter()
        .filter(|row| {
            text(row, "validAt")
                .and_then(|valid| local_date(&calendar, &valid).ok())
                .is_some_and(|date| selected.contains(date.as_str()))
        })
        .collect();
    let selected_opened: Vec<_> = opened
        .into_iter()
        .filter(|member| {
            text(member, "memberSha256").is_some_and(|hash| identities.contains(&hash))
        })
        .collect();
    let covered: HashSet<String> = selected_members
        .iter()
        .filter(|member| text(member, "recordKind").as_deref() == Some("actual_best_match"))
        .filter_map(|member| text(member, "localDate"))
        .collect();
    // require one served forecast member on every fixed epoch date
    if selected_dates.iter().any(|date| !covered.contains(date)) {
        return Err(Error::WindEpoch);
    }
    let selected_projection: Vec<_> = projection
        .projection_members
        .iter()
        .filter(|member| identities.contains(&member.member_sha256))
        .collect();
    let mut receipts: Vec<_> = selected_projection
        .iter()
        .map(|member| member.receipt.receiptSha256.clone())
        .collect();
    receipts.sort();
    let mut source_lineages: Vec<_> = selected_projection
        .iter()
        .map(|member| {
            field(&member.document, "source")
                .cloned()
                .ok_or(Error::Lineage)
        })
        .collect::<Result<_, _>>()?;
    source_lineages.sort_by_cached_key(canonical_bytes);
    let coordinate_rows = selected_members
        .iter()
        .map(|member| {
            Ok(JsonValue::Array(vec![
                string(&text(member, "localDate").ok_or(Error::Codec)?),
                string(&text(member, "recordKind").ok_or(Error::Codec)?),
                string(&text(member, "sha256").ok_or(Error::Codec)?),
            ]))
        })
        .collect::<Result<Vec<_>, Error>>()?;
    let total_row_count = selected_members.len();
    let manifest = object(vec![
        string_field(
            "aggregationContractSha256",
            &hash_json(&JsonValue::Array(
                receipts.iter().map(|item| string(item)).collect(),
            ))?,
        ),
        string_field("contractVersion", "adjustment-wind-archive-fit-manifest/v2"),
        string_field(
            "coordinateManifestSha256",
            &hash_json(&JsonValue::Array(coordinate_rows))?,
        ),
        string_field("epochWitnessSha256", witness_sha256),
        string_field("fromLocalDate", &selected_dates[0]),
        string_field("historyRootSha256", &projection.history_root_sha256),
        (
            "members".encode_utf16().collect(),
            JsonValue::Array(selected_members),
        ),
        string_field(
            "metricEligibilitySha256",
            &hash_json(&JsonValue::Array(vec![
                string("windGustMps"),
                string("windSpeedMps"),
            ]))?,
        ),
        string_field(
            "sourceLineageSha256",
            &hash_json(&JsonValue::Array(source_lineages))?,
        ),
        string_field("spatialWeightsSha256", &hash_json(&spatial_weights())?),
        string_field("stationManifestSha256", &hash_json(&station_manifest())?),
        string_field("toLocalDate", selected_dates.last().ok_or(Error::Calendar)?),
        number_field("totalRowCount", total_row_count as f64),
    ]);
    let snapshot_manifest_sha256 = hash_json(&manifest)?;
    Ok(object(vec![
        string_field("contractVersion", WIND_FIT_VERSION),
        string_field("dueMonth", due_month),
        ("manifest".encode_utf16().collect(), manifest),
        (
            "openedMembers".encode_utf16().collect(),
            JsonValue::Array(selected_opened),
        ),
        (
            "rows".encode_utf16().collect(),
            JsonValue::Array(selected_rows),
        ),
        string_field("snapshotManifestSha256", &snapshot_manifest_sha256),
    ]))
}

/// Map one actual best-match row to the immutable wind forecast shape.
fn wind_forecast_row(
    member: &NativeMaintenanceHistoricalFitMember,
) -> Result<JsonValue, NativeMaintenanceMonthlyFitError> {
    use NativeMaintenanceMonthlyFitError as Error;

    let source = field(&member.document, "source").ok_or(Error::Lineage)?;
    let reference_at = nested_text(&member.document, "logicalKey", "productRunAt")?;
    let valid_at = text(&member.row, "validAt").ok_or(Error::Input)?;
    let target_lead = ((instant(&NativeWirePrimitives, &valid_at)?
        - instant(&NativeWirePrimitives, &reference_at)?) as f64
        / HOUR_MILLISECONDS as f64)
        .ceil();
    Ok(object(vec![
        string_array_field(
            "adapterContracts",
            &[text(source, "adapterVersion").ok_or(Error::Lineage)?],
        ),
        string_field(
            "adapterVersion",
            &text(source, "adapterVersion").ok_or(Error::Lineage)?,
        ),
        number_field("collisionCount", 0.0),
        string_array_field(
            "contentHashes",
            &[text(&member.row, "contentSha256").ok_or(Error::Input)?],
        ),
        string_field(
            "contractEpoch",
            &text(source, "contractEpoch").ok_or(Error::Lineage)?,
        ),
        string_field("dataset", &text(source, "dataset").ok_or(Error::Lineage)?),
        (
            "exclusionReasonCodes".encode_utf16().collect(),
            JsonValue::Array(Vec::new()),
        ),
        string_array_field(
            "ingestionRunIds",
            std::slice::from_ref(&member.receipt.archiveCommitOrdinal),
        ),
        (
            "metrics".encode_utf16().collect(),
            wind_metrics(&member.row)?,
        ),
        (
            "physicalStationKey".encode_utf16().collect(),
            JsonValue::Null,
        ),
        ("providerFamily".encode_utf16().collect(), JsonValue::Null),
        string_field("receivedAt", &member.receipt.archiveCommittedAt),
        string_field("recordKind", "legacy_v4_retrieval_snapshot"),
        string_field("referenceAt", &reference_at),
        string_field("referenceKind", "retrieval_snapshot"),
        string_field("siteKey", "ballydidean"),
        string_array_field(
            "sourceConfigFingerprints",
            &[text(source, "sourceConfigFingerprint").ok_or(Error::Lineage)?],
        ),
        string_array_field(
            "sourceKeys",
            &[text(source, "sourceKey").ok_or(Error::Lineage)?],
        ),
        number_field("targetLeadHours", target_lead),
        string_field(
            "upstreamModel",
            &text(source, "upstreamModel").ok_or(Error::Lineage)?,
        ),
        string_field("validAt", &valid_at),
    ]))
}

/// Map one physical target through the frozen source-lineage table.
fn wind_target_row(
    member: &NativeMaintenanceHistoricalFitMember,
) -> Result<JsonValue, NativeMaintenanceMonthlyFitError> {
    use NativeMaintenanceMonthlyFitError as Error;

    let source = field(&member.document, "source").ok_or(Error::Lineage)?;
    let source_key = text(source, "sourceKey").ok_or(Error::Lineage)?;
    let lineage = source_lineage(&source_key).ok_or(Error::Lineage)?;
    if text(source, "adapterVersion").as_deref() != Some(lineage.adapter_contract)
        || text(source, "sourceConfigFingerprint").as_deref() != Some(lineage.fingerprint)
    {
        return Err(Error::Lineage);
    }
    let station = retained_forecast_observation::STATIONS
        .iter()
        .find(|station| station.key == lineage.station)
        .ok_or(Error::Lineage)?;
    Ok(object(vec![
        string_array_field("adapterContracts", &[lineage.adapter_contract.to_owned()]),
        number_field("collisionCount", 0.0),
        string_array_field(
            "contentHashes",
            &[text(&member.row, "contentSha256").ok_or(Error::Input)?],
        ),
        string_field(
            "contractEpoch",
            &text(source, "contractEpoch").ok_or(Error::Lineage)?,
        ),
        ("dataset".encode_utf16().collect(), JsonValue::Null),
        (
            "exclusionReasonCodes".encode_utf16().collect(),
            JsonValue::Array(Vec::new()),
        ),
        string_array_field(
            "ingestionRunIds",
            std::slice::from_ref(&member.receipt.archiveCommitOrdinal),
        ),
        (
            "metrics".encode_utf16().collect(),
            wind_metrics(&member.row)?,
        ),
        string_field("physicalStationKey", station_key(station.key)),
        string_field("providerFamily", provider_family(station.provider_family)),
        string_field("receivedAt", &member.receipt.archiveCommittedAt),
        string_field("recordKind", "station_hour"),
        ("referenceAt".encode_utf16().collect(), JsonValue::Null),
        ("referenceKind".encode_utf16().collect(), JsonValue::Null),
        string_field("siteKey", "ballydidean"),
        string_array_field(
            "sourceConfigFingerprints",
            &[lineage.fingerprint.to_owned()],
        ),
        string_array_field("sourceKeys", &[lineage.source_key.to_owned()]),
        ("targetLeadHours".encode_utf16().collect(), JsonValue::Null),
        ("upstreamModel".encode_utf16().collect(), JsonValue::Null),
        string_field(
            "validAt",
            &text(&member.row, "validAt").ok_or(Error::Input)?,
        ),
    ]))
}

/// Decode the five bounded fitter metrics from canonical binary64 values.
fn wind_metrics(row: &JsonValue) -> Result<JsonValue, NativeMaintenanceMonthlyFitError> {
    Ok(object(vec![
        optional_number_field(
            "relativeHumidityPercent",
            optional_binary64(row, "relativeHumidityPercent64")?,
        ),
        optional_number_field("temperatureC", optional_binary64(row, "temperatureC64")?),
        optional_number_field(
            "windDirectionDegrees",
            optional_binary64(row, "windDirectionDegrees64")?,
        ),
        optional_number_field("windGustMps", optional_binary64(row, "windGustMps64")?),
        optional_number_field("windSpeedMps", optional_binary64(row, "windSpeedMps64")?),
    ]))
}

struct SourceLineage {
    source_key: &'static str,
    adapter_contract: &'static str,
    fingerprint: &'static str,
    station: ForecastObservationStationKey,
    start: Option<&'static str>,
    end: Option<&'static str>,
}

/// Return one exact frozen source lineage by its active source key.
fn source_lineage(source_key: &str) -> Option<&'static SourceLineage> {
    SOURCE_LINEAGES
        .iter()
        .find(|lineage| lineage.source_key == source_key)
}

/// Enforce one lineage's reviewed inclusive start and exclusive end.
fn admit_lineage_clock(
    valid_at: &str,
    lineage: &SourceLineage,
) -> Result<(), NativeMaintenanceMonthlyFitError> {
    let wire = NativeWirePrimitives;
    let value = instant(&wire, valid_at)?;
    // reject target rows outside the exact source interval
    if lineage
        .start
        .is_some_and(|start| loose_instant(&wire, start).is_none_or(|start| value < start))
        || lineage
            .end
            .is_some_and(|end| loose_instant(&wire, end).is_none_or(|end| value >= end))
    {
        return Err(NativeMaintenanceMonthlyFitError::Lineage);
    }
    Ok(())
}

/// Build the exact station roster JSON consumed by the source manifest hash.
fn station_manifest() -> JsonValue {
    JsonValue::Array(
        retained_forecast_observation::station_catalog()
            .iter()
            .map(|station| {
                object(vec![
                    string_array_field("acceptedSourceKeys", &station.acceptedSourceKeys),
                    number_field("distanceMeters", station.distanceMeters),
                    (
                        "eligibleMetrics".encode_utf16().collect(),
                        JsonValue::Array(
                            station
                                .eligibleMetrics
                                .iter()
                                .map(|metric| string(metric_name(*metric)))
                                .collect(),
                        ),
                    ),
                    string_field("key", station_key(station.key)),
                    number_field("latitude", station.latitude),
                    number_field("longitude", station.longitude),
                    number_field("nearestRank", station.nearestRank as f64),
                    string_field("providerFamily", provider_family(station.providerFamily)),
                    number_field(
                        "unnormalizedSpatialWeight",
                        station.unnormalizedSpatialWeight,
                    ),
                ])
            })
            .collect(),
    )
}

/// Build the exact reduced station spatial-weight identity.
fn spatial_weights() -> JsonValue {
    JsonValue::Array(
        retained_forecast_observation::STATIONS
            .iter()
            .map(|station| {
                object(vec![
                    string_field("key", station_key(station.key)),
                    number_field("nearestRank", station.nearest_rank as f64),
                    number_field(
                        "unnormalizedSpatialWeight",
                        station.unnormalized_spatial_weight,
                    ),
                ])
            })
            .collect(),
    )
}

/// Enumerate one fixed trailing Gregorian local-date range.
fn trailing_local_dates(
    end: &str,
    count: usize,
) -> Result<Vec<String>, NativeMaintenanceMonthlyFitError> {
    let end: Date = end
        .parse()
        .map_err(|_| NativeMaintenanceMonthlyFitError::Calendar)?;
    (0..count)
        .map(|index| {
            let days = i64::try_from(count - index - 1)
                .map_err(|_| NativeMaintenanceMonthlyFitError::Calendar)?;
            end.checked_sub(Span::new().days(days))
                .map(|date| date.to_string())
                .map_err(|_| NativeMaintenanceMonthlyFitError::Calendar)
        })
        .collect()
}

/// Resolve the exact predecessor LA date for one UTC instant.
fn local_date(
    calendar: &NativePredecessorModelCalendar,
    value: &str,
) -> Result<String, NativeMaintenanceMonthlyFitError> {
    calendar
        .features(value)
        .map(|features| features.local_date)
        .map_err(|_| NativeMaintenanceMonthlyFitError::Calendar)
}

/// Decode one canonical binary64 bit string into a finite runtime value.
fn decode_binary64(value: &str) -> Result<f64, NativeMaintenanceMonthlyFitError> {
    if value.len() != 16 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(NativeMaintenanceMonthlyFitError::Numeric);
    }
    let bits =
        u64::from_str_radix(value, 16).map_err(|_| NativeMaintenanceMonthlyFitError::Numeric)?;
    let value = f64::from_bits(bits);
    value
        .is_finite()
        .then_some(value)
        .ok_or(NativeMaintenanceMonthlyFitError::Numeric)
}

/// Decode one nullable canonical binary64 field.
fn optional_binary64(
    value: &JsonValue,
    name: &str,
) -> Result<Option<f64>, NativeMaintenanceMonthlyFitError> {
    optional_text(value, name)?
        .map(|value| decode_binary64(&value))
        .transpose()
}

/// Calculate one deterministic numeric median.
fn median(mut values: Vec<f64>) -> Result<f64, NativeMaintenanceMonthlyFitError> {
    if values.is_empty() || values.iter().any(|value| !value.is_finite()) {
        return Err(NativeMaintenanceMonthlyFitError::Numeric);
    }
    values.sort_by(|left, right| left.partial_cmp(right).unwrap_or(Ordering::Equal));
    let middle = values.len() / 2;
    // average the two center values for an even population
    if values.len().is_multiple_of(2) {
        Ok((values[middle - 1] + values[middle]) / 2.0)
    } else {
        Ok(values[middle])
    }
}

/// Retain the original signed rolling-state cap.
fn clip_temperature_state(value: f64) -> f64 {
    value.clamp(-6.0, 6.0)
}

/// Parse one exact millisecond UTC instant.
fn instant(
    wire: &NativeWirePrimitives,
    value: &str,
) -> Result<i64, NativeMaintenanceMonthlyFitError> {
    let milliseconds = wire
        .parse_explicit_utc_milliseconds(value)
        .ok_or(NativeMaintenanceMonthlyFitError::Calendar)?;
    (wire.format_utc_milliseconds(milliseconds).as_deref() == Some(value))
        .then_some(milliseconds)
        .ok_or(NativeMaintenanceMonthlyFitError::Calendar)
}

/// Parse source lineage clocks that omit only the zero millisecond field.
fn loose_instant(wire: &NativeWirePrimitives, value: &str) -> Option<i64> {
    wire.parse_explicit_utc_milliseconds(value)
}

/// Hash recursively sorted canonical JSON with its terminating line feed.
fn hash_json(value: &JsonValue) -> Result<String, NativeMaintenanceMonthlyFitError> {
    let wire = NativeWirePrimitives;
    let bytes = wire
        .encode_sorted_json(value)
        .ok_or(NativeMaintenanceMonthlyFitError::Codec)?;
    Ok(format_digest(wire.sha256(&bytes)))
}

/// Encode producer order without the line feed used by `JSON.stringify` state hashing.
fn ordered_json_sha256(value: &JsonValue) -> Result<String, NativeMaintenanceMonthlyFitError> {
    let mut bytes =
        NativeJson::encode_ordered(value).ok_or(NativeMaintenanceMonthlyFitError::Codec)?;
    // remove exactly the native encoder's terminating line feed
    if bytes.pop() != Some(b'\n') {
        return Err(NativeMaintenanceMonthlyFitError::Codec);
    }
    Ok(format_digest(NativeWirePrimitives.sha256(&bytes)))
}

/// Encode recursively sorted canonical bytes for deterministic comparisons.
fn canonical_bytes(value: &JsonValue) -> Vec<u8> {
    NativeWirePrimitives
        .encode_sorted_json(value)
        .expect("historical projection already admitted canonical JSON")
}

/// Compare uint64 decimal text without numeric coercion.
fn compare_ordinal(left: &str, right: &str) -> Ordering {
    left.len().cmp(&right.len()).then_with(|| left.cmp(right))
}

/// Require one exact nested string field.
fn nested_text(
    value: &JsonValue,
    object_name: &str,
    field_name: &str,
) -> Result<String, NativeMaintenanceMonthlyFitError> {
    field(value, object_name)
        .and_then(|object| text(object, field_name))
        .ok_or(NativeMaintenanceMonthlyFitError::Input)
}

/// Decode one nested nullable string field.
fn nested_optional_text(
    value: &JsonValue,
    object_name: &str,
    field_name: &str,
) -> Result<Option<String>, NativeMaintenanceMonthlyFitError> {
    let object = field(value, object_name).ok_or(NativeMaintenanceMonthlyFitError::Input)?;
    optional_text(object, field_name)
}

/// Decode one absent, null, or string JSON field.
fn optional_text(
    value: &JsonValue,
    name: &str,
) -> Result<Option<String>, NativeMaintenanceMonthlyFitError> {
    // preserve JavaScript null and absent values without coercion
    match field(value, name) {
        None | Some(JsonValue::Null) => Ok(None),
        Some(JsonValue::String(units)) => String::from_utf16(units)
            .map(Some)
            .map_err(|_| NativeMaintenanceMonthlyFitError::Input),
        Some(_) => Err(NativeMaintenanceMonthlyFitError::Input),
    }
}

/// Decode one safe unsigned JSON number.
fn unsigned(value: &JsonValue, name: &str) -> Result<u64, NativeMaintenanceMonthlyFitError> {
    let Some(JsonValue::Number(value)) = field(value, name) else {
        return Err(NativeMaintenanceMonthlyFitError::Numeric);
    };
    if !value.is_finite()
        || value.fract() != 0.0
        || *value < 0.0
        || *value > 9_007_199_254_740_991.0
    {
        return Err(NativeMaintenanceMonthlyFitError::Numeric);
    }
    Ok(*value as u64)
}

/// Require the source's exact fit-month spelling.
fn valid_month(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 7
        && bytes.starts_with(b"20")
        && bytes[4] == b'-'
        && bytes[2..4].iter().all(u8::is_ascii_digit)
        && matches!(
            (bytes[5], bytes[6]),
            (b'0', b'1'..=b'9') | (b'1', b'0'..=b'2')
        )
}

/// Require the source's exact 20xx millisecond UTC spelling.
fn valid_instant(value: &str) -> bool {
    value.len() == 24
        && value.starts_with("20")
        && value.as_bytes().get(19) == Some(&b'.')
        && value.ends_with('Z')
        && instant(&NativeWirePrimitives, value).is_ok()
}

/// Return the exact station-key wire spelling.
fn station_key(value: ForecastObservationStationKey) -> &'static str {
    ForecastObservationStationKey::WIRE_VALUES
        .iter()
        .find_map(|(wire, candidate)| (*candidate == value).then_some(*wire))
        .expect("generated station key")
}

/// Return the exact provider-family wire spelling.
fn provider_family(value: ForecastObservationProviderFamily) -> &'static str {
    ForecastObservationProviderFamily::WIRE_VALUES
        .iter()
        .find_map(|(wire, candidate)| (*candidate == value).then_some(*wire))
        .expect("generated provider family")
}

/// Return the exact eligible-metric wire spelling.
fn metric_name(value: ForecastAdjustmentMetric) -> &'static str {
    ForecastAdjustmentMetric::WIRE_VALUES
        .iter()
        .find_map(|(wire, candidate)| (*candidate == value).then_some(*wire))
        .expect("generated metric")
}

/// Build one JSON object while preserving the producer field order.
fn object(fields: Vec<(Vec<u16>, JsonValue)>) -> JsonValue {
    JsonValue::Object(fields)
}

/// Build one UTF-16 JSON string.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Build one JSON string field.
fn string_field(name: &str, value: &str) -> (Vec<u16>, JsonValue) {
    (name.encode_utf16().collect(), string(value))
}

/// Build one nullable JSON string field.
fn optional_string_field(name: &str, value: Option<&str>) -> (Vec<u16>, JsonValue) {
    (
        name.encode_utf16().collect(),
        value.map(string).unwrap_or(JsonValue::Null),
    )
}

/// Build one finite JSON number field.
fn number_field(name: &str, value: f64) -> (Vec<u16>, JsonValue) {
    (name.encode_utf16().collect(), JsonValue::Number(value))
}

/// Build one nullable finite JSON number field.
fn optional_number_field(name: &str, value: Option<f64>) -> (Vec<u16>, JsonValue) {
    (
        name.encode_utf16().collect(),
        value.map(JsonValue::Number).unwrap_or(JsonValue::Null),
    )
}

/// Build one JSON string-array field.
fn string_array_field(name: &str, values: &[String]) -> (Vec<u16>, JsonValue) {
    (
        name.encode_utf16().collect(),
        JsonValue::Array(values.iter().map(|value| string(value)).collect()),
    )
}

const SOURCE_LINEAGES: &[SourceLineage] = &[
    SourceLineage {
        source_key: "ambient-maxweather-observations-v1",
        adapter_contract: "ambient-device-data/v1",
        fingerprint: "7a7528a6278924ca5280a1a6045b6647b7e660b112d7fa3008c542a17ff99df4",
        station: ForecastObservationStationKey::AmbientMaxweather,
        start: Some("2026-08-24T00:00:00Z"),
        end: None,
    },
    SourceLineage {
        source_key: "ambient-merlin-observations-v1",
        adapter_contract: "ambient-device-data/v1",
        fingerprint: "c3829701bfc25a050022dc3965569d3a87376e8a43b5fdcb7621533f1ae3c65d",
        station: ForecastObservationStationKey::AmbientMerlin,
        start: Some("2021-01-01T00:00:00Z"),
        end: None,
    },
    SourceLineage {
        source_key: "ecowitt-88f15505d89f-local-live-v1",
        adapter_contract: "ecowitt-local-live/v1",
        fingerprint: "0a44488714d0fa807b924f8aea14965b437722e8cf9f8eae4bc8c81da8a0149d",
        station: ForecastObservationStationKey::BallydideanEcowitt,
        start: None,
        end: None,
    },
    SourceLineage {
        source_key: "netatmo-nearby-observations-v1",
        adapter_contract: "netatmo-public-measures/v1",
        fingerprint: "5495917dd2465a32d9878e73c68781a229b432901cdc4867875726351efbdbbc",
        station: ForecastObservationStationKey::NetatmoNearby,
        start: Some("2022-06-21T00:00:00Z"),
        end: None,
    },
    SourceLineage {
        source_key: "tempest-126537-observations-v2",
        adapter_contract: "tempest-observations/v2",
        fingerprint: "34dafbd6584c93d55ed4d3d43dc7e74a0876165d4ddfc921413f7b826dff7ab7",
        station: ForecastObservationStationKey::Tempest126537,
        start: Some("2023-12-17T00:00:00Z"),
        end: None,
    },
    SourceLineage {
        source_key: "tempest-168853-observations-v2",
        adapter_contract: "tempest-observations/v2",
        fingerprint: "1c7a402337a44a5441775246cbc02da7994599dad6ab83dc04b248303facfea5",
        station: ForecastObservationStationKey::Tempest168853,
        start: Some("2025-01-22T00:00:00Z"),
        end: None,
    },
    SourceLineage {
        source_key: "tempest-201058-observations-v2",
        adapter_contract: "tempest-observations/v2",
        fingerprint: "a61cce798cddf682da9608dc245659fc7734a6d5304068939d3115ae7d81a50e",
        station: ForecastObservationStationKey::Tempest201058,
        start: Some("2025-12-22T00:00:00Z"),
        end: None,
    },
    SourceLineage {
        source_key: "tempest-203055-observations-v2",
        adapter_contract: "tempest-observations/v2",
        fingerprint: "9ead4c5359a6a9640f334be91397180aa62b90b0f0ce813b9ff26fe84537acc4",
        station: ForecastObservationStationKey::Tempest203055,
        start: Some("2025-12-25T00:00:00Z"),
        end: None,
    },
    SourceLineage {
        source_key: "tempest-225947-observations-v2",
        adapter_contract: "tempest-observations/v2",
        fingerprint: "b4dd6105d9a56a7c5d0dc4063f830e1cf28d693222a8de15536dd83d3a6178c4",
        station: ForecastObservationStationKey::Tempest225947,
        start: Some("2026-07-14T00:00:00Z"),
        end: None,
    },
    SourceLineage {
        source_key: "tempest-38270-observations-v2",
        adapter_contract: "tempest-observations/v2",
        fingerprint: "ce162067aced4ab3522fb83145a21e608ff24dec189097726188e96fd6cca52f",
        station: ForecastObservationStationKey::Tempest38270,
        start: Some("2021-01-04T00:00:00Z"),
        end: None,
    },
    SourceLineage {
        source_key: "tempest-64255-observations-v2",
        adapter_contract: "tempest-observations/v2",
        fingerprint: "8eb488a358375fc3526347d9ef6c9f23080095a22ea874a42ec400b0317d868a",
        station: ForecastObservationStationKey::Tempest64255,
        start: Some("2021-12-10T00:00:00Z"),
        end: None,
    },
    SourceLineage {
        source_key: "wunderground-maxweather-history-v1",
        adapter_contract: "wunderground-pws-history/v1",
        fingerprint: "52dda6c5444d0a234fbe23d6218027d417ac966ecf291a7d5dfff42fd0dc207c",
        station: ForecastObservationStationKey::AmbientMaxweather,
        start: Some("2024-11-29T00:00:00Z"),
        end: Some("2026-08-24T00:00:00Z"),
    },
];

#[cfg(test)]
#[path = "native_maintenance_monthly_fit_test.rs"]
mod tests;
