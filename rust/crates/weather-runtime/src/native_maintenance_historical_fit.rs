//! Exact verified-history projection consumed by the three maintenance fitters.

use std::cmp::Ordering;
use std::fmt::{self, Display, Formatter};
use std::sync::Arc;

use weather_contracts::{
    AdjustmentRevisionCommitReceipt, AdjustmentRevisionCommitReceiptProjectionKindEnum, JsonValue,
};
use weather_edge::{
    ArchiveWirePrimitives, PrivateShadowReceiptCodec, RevisionArchiveCanonicalCodec,
};

use crate::maintenance_canonical_json::{field, text, valid_hash};
use crate::native_maintenance_historical_catalog::{
    NativeMaintenanceHistoricalArchiveIndex, NativeMaintenanceHistoricalFamily,
    NativeMaintenanceHistoricalOccurrence,
};
use crate::native_maintenance_retirement_files_v2::format_digest;
use crate::{
    NativeModelEvidenceValues, NativePredecessorModelCalendar, NativeShadowProducerCodec,
    NativeWirePrimitives, VerifiedNativeMaintenanceCaptureEpochWitness,
    decode_native_api_revision_document,
};

pub const NATIVE_MAINTENANCE_HISTORICAL_FIT_VERSION: &str = "adjustment-revision-fit-projection/v2";

const MAXIMUM_PROJECTED_ROWS: usize = 1_000_000;
const DAY_MILLISECONDS: i64 = 86_400_000;
const SHADOW_V1: &str = "adjustment-shadow-revision-capsule/v1";
const SHADOW_V2: &str = "adjustment-shadow-revision-capsule/v2";
const FIXED_GAUGE: &str = "adjustment-rain-fixed-gauge-target-projection/v1";
const BATCH_V2: &str = "adjustment-revision-batch-projection/v2";
const HISTORY_VERSION: &str = "adjustment-revision-historical-archive-index/v1";

/// Exact counts of source projection classes retained for fitting.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NativeMaintenanceHistoricalFitClassCounts {
    pub actual_best_match: usize,
    pub native_source: usize,
    pub rain_gate_input: usize,
    pub target_revision: usize,
}

/// One exact selected row and all immutable archive lineage needed by a fitter.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceHistoricalFitMember {
    pub document: JsonValue,
    pub graph_manifest_sha256: String,
    pub member_sha256: String,
    pub payload_bytes: Arc<[u8]>,
    pub payload_identity_sha256: String,
    pub projection_kind: String,
    pub receipt: AdjustmentRevisionCommitReceipt,
    pub row: JsonValue,
    pub row_index: usize,
    sort_valid_at: String,
}

/// One same-family post-epoch shadow capsule retained as historical fit context.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceHistoricalFitShadowMember {
    pub capsule: JsonValue,
    pub graph_manifest_sha256: String,
    pub issued_at: String,
    pub payload_bytes: Arc<[u8]>,
    pub payload_identity_sha256: String,
    pub receipt: AdjustmentRevisionCommitReceipt,
}

/// Complete source projection over one verified historical archive prefix.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceHistoricalFitProjection {
    pub class_counts: NativeMaintenanceHistoricalFitClassCounts,
    pub contract_version: &'static str,
    pub cutoff_at: String,
    pub due_month: String,
    pub epoch_witness_sha256: String,
    pub family: NativeMaintenanceHistoricalFamily,
    pub history_root_sha256: String,
    pub member_root_sha256: String,
    pub projection_members: Vec<NativeMaintenanceHistoricalFitMember>,
    pub row_count: usize,
    pub shadow_members: Vec<NativeMaintenanceHistoricalFitShadowMember>,
}

/// Closed admission and projection failures for verified historical inputs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceHistoricalFitError {
    Input,
    Calendar,
    Clock,
    Occurrence,
    Payload,
    Receipt,
    Size,
    Codec,
}

impl Display for NativeMaintenanceHistoricalFitError {
    /// Render one stable diagnostic without reflecting archive material.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Input => "historical fit input is invalid",
            Self::Calendar => "historical fit cutoff calendar is invalid",
            Self::Clock => "historical fit clock predates its epoch",
            Self::Occurrence => "historical fit occurrence differs",
            Self::Payload => "historical fit payload is invalid",
            Self::Receipt => "historical fit receipt population differs",
            Self::Size => "historical fit projection row ceiling exceeded",
            Self::Codec => "historical fit canonical encoding failed",
        })
    }
}

impl std::error::Error for NativeMaintenanceHistoricalFitError {}

/// Project one verified historical archive prefix into the exact family fit population.
pub fn build_native_adjustment_historical_fit_projection(
    epoch_witness: &VerifiedNativeMaintenanceCaptureEpochWitness,
    family: NativeMaintenanceHistoricalFamily,
    fit_month: &str,
    history: &NativeMaintenanceHistoricalArchiveIndex,
) -> Result<NativeMaintenanceHistoricalFitProjection, NativeMaintenanceHistoricalFitError> {
    use NativeMaintenanceHistoricalFitError as Error;

    let wire = NativeWirePrimitives;
    let epoch_at = canonical_instant(&wire, &epoch_witness.epoch_at).ok_or(Error::Input)?;
    // require the independently verified epoch and archive index identities
    if !valid_hash(&epoch_witness.witness_sha256)
        || history.contract_version != HISTORY_VERSION
        || !valid_hash(&history.history_root_sha256)
        || !valid_month(fit_month)
    {
        return Err(Error::Input);
    }
    let cutoff_at = original_training_cutoff(&wire, family, fit_month)?;
    let cutoff = canonical_instant(&wire, &cutoff_at).ok_or(Error::Calendar)?;
    let mut projection_members = Vec::new();
    let mut shadow_members = Vec::new();
    let mut row_count = 0usize;
    // inspect only published value-bearing occurrences from the verified prefix
    for occurrence in &history.occurrences {
        if occurrence.publication_disposition != "published" {
            continue;
        }
        // retain shadow capsules through their distinct receipt and source binding
        if matches!(occurrence.payload_kind.as_str(), SHADOW_V1 | SHADOW_V2) {
            let selected = select_shadow_member(&wire, epoch_at, cutoff, family, occurrence)?;
            if let Some((member, count)) = selected {
                shadow_members.push(member);
                row_count = row_count.checked_add(count).ok_or(Error::Size)?;
            }
            continue;
        }
        let (document, rows, contract, projection_kind) =
            decode_revision_occurrence(&wire, occurrence)?;
        validate_revision_occurrence(&wire, occurrence, &contract, &projection_kind, rows.len())?;
        // omit unrelated projection classes without relabelling them
        if !projection_is_relevant(&document, &contract, &projection_kind, family) {
            continue;
        }
        let selected = select_projection_rows(
            &wire,
            epoch_at,
            cutoff,
            occurrence,
            &document,
            &contract,
            &projection_kind,
            rows,
        )?;
        row_count = row_count.checked_add(selected.len()).ok_or(Error::Size)?;
        projection_members.extend(selected);
        // reproduce the source ceiling check after each projection occurrence
        if row_count > MAXIMUM_PROJECTED_ROWS {
            return Err(Error::Size);
        }
    }
    projection_members.sort_by(compare_projected_member);
    shadow_members.sort_by(|left, right| left.issued_at.cmp(&right.issued_at));
    let mut identities: Vec<String> = projection_members
        .iter()
        .map(|member| member.member_sha256.clone())
        .chain(
            shadow_members
                .iter()
                .map(|member| member.payload_identity_sha256.clone()),
        )
        .collect();
    identities.sort();
    let member_root_sha256 = hash_json(
        &wire,
        &JsonValue::Array(identities.iter().map(|value| string(value)).collect()),
    )?;
    let mut class_counts = NativeMaintenanceHistoricalFitClassCounts::default();
    // count only the four genuine projection classes
    for member in &projection_members {
        match member.projection_kind.as_str() {
            "actual_best_match" => class_counts.actual_best_match += 1,
            "native_source" => class_counts.native_source += 1,
            "rain_gate_input" => class_counts.rain_gate_input += 1,
            "target_revision" => class_counts.target_revision += 1,
            _ => return Err(Error::Payload),
        }
    }
    Ok(NativeMaintenanceHistoricalFitProjection {
        class_counts,
        contract_version: NATIVE_MAINTENANCE_HISTORICAL_FIT_VERSION,
        cutoff_at,
        due_month: fit_month.to_owned(),
        epoch_witness_sha256: epoch_witness.witness_sha256.clone(),
        family,
        history_root_sha256: history.history_root_sha256.clone(),
        member_root_sha256,
        projection_members,
        row_count,
        shadow_members,
    })
}

/// Decode and retain one canonical non-shadow revision occurrence.
fn decode_revision_occurrence(
    wire: &NativeWirePrimitives,
    occurrence: &NativeMaintenanceHistoricalOccurrence,
) -> Result<(JsonValue, Vec<JsonValue>, String, String), NativeMaintenanceHistoricalFitError> {
    use NativeMaintenanceHistoricalFitError::Payload;

    decode_native_api_revision_document(&occurrence.payload_bytes).ok_or(Payload)?;
    let document = wire.decode_json(&occurrence.payload_bytes).ok_or(Payload)?;
    let contract = text(&document, "contractVersion").ok_or(Payload)?;
    let projection_kind = text(&document, "projectionKind").ok_or(Payload)?;
    let JsonValue::Array(rows) = field(&document, "rows").ok_or(Payload)? else {
        return Err(Payload);
    };
    let rows = rows.clone();
    Ok((document, rows, contract, projection_kind))
}

/// Bind one archived revision payload to its exact ordered receipt population.
fn validate_revision_occurrence(
    wire: &NativeWirePrimitives,
    occurrence: &NativeMaintenanceHistoricalOccurrence,
    contract: &str,
    projection_kind: &str,
    row_count: usize,
) -> Result<(), NativeMaintenanceHistoricalFitError> {
    use NativeMaintenanceHistoricalFitError::{Occurrence, Receipt};

    let expected_receipts = if matches!(contract, BATCH_V2 | FIXED_GAUGE) {
        row_count
    } else {
        1
    };
    // bind payload kind, bytes, and the singular or per-row receipt population
    if occurrence.payload_kind != contract
        || digest(wire, &occurrence.payload_bytes) != occurrence.payload_identity_sha256
    {
        return Err(Occurrence);
    }
    if occurrence.receipts.len() != expected_receipts
        || occurrence.receipts.iter().any(|receipt| {
            receipt_kind(receipt.projectionKind) != projection_kind
                || receipt.projectionIdentitySha256 != occurrence.payload_identity_sha256
                || receipt.projectionSha256 != occurrence.payload_identity_sha256
        })
    {
        return Err(Receipt);
    }
    Ok(())
}

/// Select one same-family post-epoch shadow capsule.
fn select_shadow_member(
    wire: &NativeWirePrimitives,
    epoch_at: i64,
    cutoff: i64,
    family: NativeMaintenanceHistoricalFamily,
    occurrence: &NativeMaintenanceHistoricalOccurrence,
) -> Result<
    Option<(NativeMaintenanceHistoricalFitShadowMember, usize)>,
    NativeMaintenanceHistoricalFitError,
> {
    use NativeMaintenanceHistoricalFitError::{Occurrence, Payload, Receipt, Size};

    let codec = NativeShadowProducerCodec::new(NativeModelEvidenceValues::new());
    let capsule =
        PrivateShadowReceiptCodec::decode_capsule_document(&codec, &occurrence.payload_bytes)
            .ok_or(Payload)?;
    let capsule_value = wire.decode_json(&occurrence.payload_bytes).ok_or(Payload)?;
    let source = wire
        .decode_json(&capsule.stage_input.sourceProjection)
        .ok_or(Payload)?;
    let source_family = text(&source, "family").ok_or(Payload)?;
    let issued_at = capsule.stage_input.metadata.issuedAt.clone();
    let issued = require_post_epoch(wire, &issued_at, epoch_at)?;
    // keep only same-family capsules earlier than the immutable cutoff
    if source_family != family.as_str() || issued >= cutoff {
        return Ok(None);
    }
    if occurrence.receipts.len() != 1
        || occurrence.receipts[0].projectionKind
            != AdjustmentRevisionCommitReceiptProjectionKindEnum::ShadowPrediction
        || occurrence.receipts[0] != capsule.revision_receipt
    {
        return Err(Receipt);
    }
    if digest(wire, &occurrence.payload_bytes) != occurrence.payload_identity_sha256 {
        return Err(Occurrence);
    }
    let row_count = usize::try_from(capsule.stage_input.metadata.rowCount).map_err(|_| Size)?;
    Ok(Some((
        NativeMaintenanceHistoricalFitShadowMember {
            capsule: capsule_value,
            graph_manifest_sha256: occurrence.graph_manifest_sha256.clone(),
            issued_at,
            payload_bytes: Arc::from(occurrence.payload_bytes.clone()),
            payload_identity_sha256: occurrence.payload_identity_sha256.clone(),
            receipt: occurrence.receipts[0].clone(),
        },
        row_count,
    )))
}

/// Select every family-relevant row whose full causal lineage precedes the cutoff.
#[allow(clippy::too_many_arguments)]
fn select_projection_rows(
    wire: &NativeWirePrimitives,
    epoch_at: i64,
    cutoff: i64,
    occurrence: &NativeMaintenanceHistoricalOccurrence,
    document: &JsonValue,
    contract: &str,
    projection_kind: &str,
    rows: Vec<JsonValue>,
) -> Result<Vec<NativeMaintenanceHistoricalFitMember>, NativeMaintenanceHistoricalFitError> {
    use NativeMaintenanceHistoricalFitError::{Payload, Receipt};

    let fixed_gauge = contract == FIXED_GAUGE;
    let payload_bytes: Arc<[u8]> = Arc::from(occurrence.payload_bytes.clone());
    // admit the twelve-source target atomically at its last native receipt
    if fixed_gauge {
        for receipt in &occurrence.receipts {
            if require_post_epoch(wire, &receipt.archiveCommittedAt, epoch_at)? >= cutoff {
                return Ok(Vec::new());
            }
        }
    }
    let mut selected = Vec::new();
    // bind grouped rows to their exact database ordinal receipts
    for (row_index, row) in rows.into_iter().enumerate() {
        let receipt = if occurrence.receipts.len() == 1 {
            occurrence.receipts.first()
        } else {
            occurrence.receipts.get(row_index)
        }
        .ok_or(Receipt)?;
        let clocks = revision_row_clocks(document, &row, fixed_gauge)?;
        let mut crosses_cutoff = false;
        // require every causal source, target, and receipt clock after the epoch
        for clock in clocks {
            if require_post_epoch(wire, &clock, epoch_at)? >= cutoff {
                crosses_cutoff = true;
            }
        }
        if require_post_epoch(wire, &receipt.archiveCommittedAt, epoch_at)? >= cutoff {
            crosses_cutoff = true;
        }
        if crosses_cutoff {
            continue;
        }
        let identity = JsonValue::Object(vec![
            string_field("archiveCommitOrdinal", &receipt.archiveCommitOrdinal),
            string_field("payloadIdentitySha256", &occurrence.payload_identity_sha256),
            string_field("projectionKind", projection_kind),
            string_field("receiptSha256", &receipt.receiptSha256),
            (
                "rowIndex".encode_utf16().collect(),
                JsonValue::Number(row_index as f64),
            ),
        ]);
        let member_sha256 = hash_json(wire, &identity)?;
        let sort_valid_at = projected_valid_at(&row, fixed_gauge).ok_or(Payload)?;
        selected.push(NativeMaintenanceHistoricalFitMember {
            document: document.clone(),
            graph_manifest_sha256: occurrence.graph_manifest_sha256.clone(),
            member_sha256,
            payload_bytes: Arc::clone(&payload_bytes),
            payload_identity_sha256: occurrence.payload_identity_sha256.clone(),
            projection_kind: projection_kind.to_owned(),
            receipt: receipt.clone(),
            row,
            row_index,
            sort_valid_at,
        });
    }
    Ok(selected)
}

/// Enumerate every source clock independently enforcing future-only lineage.
fn revision_row_clocks(
    document: &JsonValue,
    row: &JsonValue,
    fixed_gauge: bool,
) -> Result<Vec<String>, NativeMaintenanceHistoricalFitError> {
    use NativeMaintenanceHistoricalFitError::Payload;

    // retain every consumed gauge claim, target, and interval clock
    if fixed_gauge {
        let normalized = field(row, "normalizedRecord").ok_or(Payload)?;
        let mut clocks = vec![
            text(document, "logicalReceivedAt").ok_or(Payload)?,
            text(document, "validAt").ok_or(Payload)?,
            text(normalized, "validAt").ok_or(Payload)?,
            text(normalized, "receivedAt").ok_or(Payload)?,
        ];
        for body in array(document, "captureBodies")? {
            for claim in array(body, "claims")? {
                clocks.extend([
                    text(claim, "completedAt").ok_or(Payload)?,
                    text(claim, "windowStart").ok_or(Payload)?,
                    text(claim, "windowEndExclusive").ok_or(Payload)?,
                ]);
            }
        }
        for interval in array(row, "intervals")? {
            clocks.extend([
                text(interval, "validAt").ok_or(Payload)?,
                text(interval, "completedAt").ok_or(Payload)?,
            ]);
        }
        return Ok(clocks);
    }
    let mut clocks = vec![
        text(document, "logicalReceivedAt").ok_or(Payload)?,
        text(row, "validAt").ok_or(Payload)?,
    ];
    // include the exact native comparator initialization when present
    if let Some(value) = optional_text(row, "bestMatchProductRunAt")? {
        clocks.push(value);
    }
    let logical_key = field(document, "logicalKey").ok_or(Payload)?;
    // include each kind-specific source clock without inferring absent fields
    for name in ["productRunAt", "runInitializedAt"] {
        if let Some(value) = optional_text(logical_key, name)? {
            clocks.push(value);
        }
    }
    Ok(clocks)
}

/// Decide whether one admitted producer class is consumed by the selected fitter.
fn projection_is_relevant(
    document: &JsonValue,
    contract: &str,
    projection_kind: &str,
    family: NativeMaintenanceHistoricalFamily,
) -> bool {
    // every family consumes only its physical target representation
    if projection_kind == "target_revision" {
        return if family == NativeMaintenanceHistoricalFamily::Rain {
            contract == FIXED_GAUGE
        } else {
            contract != FIXED_GAUGE
        };
    }
    // preserve the source's intentionally distinct family class sets
    match family {
        NativeMaintenanceHistoricalFamily::Temperature => {
            projection_kind == "actual_best_match"
                || projection_kind == "native_source"
                    && text(document, "family").as_deref() == Some("temperature")
        }
        NativeMaintenanceHistoricalFamily::Wind => {
            matches!(projection_kind, "native_source" | "actual_best_match")
        }
        NativeMaintenanceHistoricalFamily::Rain => matches!(
            projection_kind,
            "native_source" | "rain_gate_input" | "actual_best_match"
        ),
    }
}

/// Order selected rows by valid clock, uint64 ordinal, and row position.
fn compare_projected_member(
    left: &NativeMaintenanceHistoricalFitMember,
    right: &NativeMaintenanceHistoricalFitMember,
) -> Ordering {
    left.sort_valid_at
        .cmp(&right.sort_valid_at)
        .then_with(|| {
            compare_ordinal(
                &left.receipt.archiveCommitOrdinal,
                &right.receipt.archiveCommitOrdinal,
            )
        })
        .then_with(|| left.row_index.cmp(&right.row_index))
}

/// Extract the source ordering clock from an ordinary or fixed-gauge row.
fn projected_valid_at(row: &JsonValue, fixed_gauge: bool) -> Option<String> {
    // fixed gauge orders by its normalized target clock
    if fixed_gauge {
        return field(row, "normalizedRecord").and_then(|value| text(value, "validAt"));
    }
    text(row, "validAt")
}

/// Compare canonical uint64 decimal text without numeric coercion.
fn compare_ordinal(left: &str, right: &str) -> Ordering {
    left.len().cmp(&right.len()).then_with(|| left.cmp(right))
}

/// Derive the immutable family cutoff with the predecessor LA calendar.
fn original_training_cutoff(
    wire: &NativeWirePrimitives,
    family: NativeMaintenanceHistoricalFamily,
    fit_month: &str,
) -> Result<String, NativeMaintenanceHistoricalFitError> {
    use NativeMaintenanceHistoricalFitError::Calendar;

    let local_date = format!("{fit_month}-01");
    let month_start = if family == NativeMaintenanceHistoricalFamily::Rain {
        canonical_instant(wire, &format!("{local_date}T00:00:00.000Z")).ok_or(Calendar)?
    } else {
        local_midnight(wire, &local_date)?
    };
    let cutoff = month_start
        .checked_sub(7 * DAY_MILLISECONDS)
        .ok_or(Calendar)?;
    wire.format_utc_milliseconds(cutoff).ok_or(Calendar)
}

/// Resolve LA midnight using the source's three wall-clock corrections.
fn local_midnight(
    wire: &NativeWirePrimitives,
    local_date: &str,
) -> Result<i64, NativeMaintenanceHistoricalFitError> {
    use NativeMaintenanceHistoricalFitError::Calendar;

    let calendar = NativePredecessorModelCalendar::new().ok_or(Calendar)?;
    let target = canonical_instant(wire, &format!("{local_date}T00:00:00.000Z")).ok_or(Calendar)?;
    let mut guess =
        canonical_instant(wire, &format!("{local_date}T08:00:00.000Z")).ok_or(Calendar)?;
    // reproduce the predecessor's bounded Intl wall-clock correction
    for _ in 0..3 {
        let instant = wire.format_utc_milliseconds(guess).ok_or(Calendar)?;
        let offset = calendar
            .features(&instant)
            .map_err(|_| Calendar)?
            .offset_seconds;
        let observed = guess
            .checked_add(i64::from(offset) * 1_000)
            .ok_or(Calendar)?;
        guess = guess
            .checked_add(target.checked_sub(observed).ok_or(Calendar)?)
            .ok_or(Calendar)?;
    }
    let resolved = wire.format_utc_milliseconds(guess).ok_or(Calendar)?;
    if calendar
        .features(&resolved)
        .map_err(|_| Calendar)?
        .local_date
        != local_date
    {
        return Err(Calendar);
    }
    Ok(guess)
}

/// Require one canonical source clock at or after the frozen epoch.
fn require_post_epoch(
    wire: &NativeWirePrimitives,
    value: &str,
    epoch_at: i64,
) -> Result<i64, NativeMaintenanceHistoricalFitError> {
    let instant =
        canonical_instant(wire, value).ok_or(NativeMaintenanceHistoricalFitError::Clock)?;
    // reject pre-epoch causal history instead of repairing it
    if instant < epoch_at {
        return Err(NativeMaintenanceHistoricalFitError::Clock);
    }
    Ok(instant)
}

/// Parse only exact canonical millisecond UTC source instants.
fn canonical_instant(wire: &NativeWirePrimitives, value: &str) -> Option<i64> {
    // retain the source's exact 20xx millisecond UTC grammar
    if value.len() != 24
        || !value.starts_with("20")
        || value.as_bytes().get(19) != Some(&b'.')
        || !value.ends_with('Z')
    {
        return None;
    }
    let milliseconds = wire.parse_explicit_utc_milliseconds(value)?;
    (wire.format_utc_milliseconds(milliseconds)?.as_str() == value).then_some(milliseconds)
}

/// Require the source's four-digit canonical fit-month spelling.
fn valid_month(value: &str) -> bool {
    let bytes = value.as_bytes();
    // retain only 2000 through 2099 and canonical month fields
    bytes.len() == 7
        && bytes.starts_with(b"20")
        && bytes[4] == b'-'
        && bytes[2..4].iter().all(u8::is_ascii_digit)
        && matches!(
            (bytes[5], bytes[6]),
            (b'0', b'1'..=b'9') | (b'1', b'0'..=b'2')
        )
}

/// Borrow one named JSON array.
fn array<'a>(
    value: &'a JsonValue,
    name: &str,
) -> Result<&'a [JsonValue], NativeMaintenanceHistoricalFitError> {
    let Some(JsonValue::Array(values)) = field(value, name) else {
        return Err(NativeMaintenanceHistoricalFitError::Payload);
    };
    Ok(values)
}

/// Decode a missing, null, or string field without inventing a value.
fn optional_text(
    value: &JsonValue,
    name: &str,
) -> Result<Option<String>, NativeMaintenanceHistoricalFitError> {
    // preserve source null and absent fields as the same optional clock
    match field(value, name) {
        None | Some(JsonValue::Null) => Ok(None),
        Some(JsonValue::String(units)) => String::from_utf16(units)
            .map(Some)
            .map_err(|_| NativeMaintenanceHistoricalFitError::Payload),
        Some(_) => Err(NativeMaintenanceHistoricalFitError::Payload),
    }
}

/// Return the exact generated receipt projection-kind spelling.
fn receipt_kind(value: AdjustmentRevisionCommitReceiptProjectionKindEnum) -> &'static str {
    // keep the generated closed enum aligned with source wire text
    match value {
        AdjustmentRevisionCommitReceiptProjectionKindEnum::ActualBestMatch => "actual_best_match",
        AdjustmentRevisionCommitReceiptProjectionKindEnum::NativeSource => "native_source",
        AdjustmentRevisionCommitReceiptProjectionKindEnum::RainGateInput => "rain_gate_input",
        AdjustmentRevisionCommitReceiptProjectionKindEnum::ShadowPrediction => "shadow_prediction",
        AdjustmentRevisionCommitReceiptProjectionKindEnum::TargetRevision => "target_revision",
    }
}

/// Hash one recursively sorted canonical JSON value.
fn hash_json(
    wire: &NativeWirePrimitives,
    value: &JsonValue,
) -> Result<String, NativeMaintenanceHistoricalFitError> {
    let bytes = wire
        .encode_sorted_json(value)
        .ok_or(NativeMaintenanceHistoricalFitError::Codec)?;
    Ok(digest(wire, &bytes))
}

/// Hash exact retained bytes with the installed native primitive.
fn digest(wire: &NativeWirePrimitives, bytes: &[u8]) -> String {
    format_digest(wire.sha256(bytes))
}

/// Build one UTF-16 JSON string.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Build one UTF-16 JSON string field.
fn string_field(name: &str, value: &str) -> (Vec<u16>, JsonValue) {
    (name.encode_utf16().collect(), string(value))
}

// compile the retained predecessor projection fixture with the native owner
#[cfg(test)]
#[path = "native_maintenance_historical_fit_test.rs"]
mod tests;
