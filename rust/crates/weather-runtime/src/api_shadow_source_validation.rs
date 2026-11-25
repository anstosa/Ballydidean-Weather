//! Validate canonical private shadow source projections before identity derivation.

use weather_contracts::JsonValue;

use crate::api_maintenance_receipts::ApiMaintenanceReceiptCanonicalValues;
use crate::api_shadow_rain_source_validation::validate_rain_causal_inputs;
use crate::api_shadow_wire_validation::{
    HOUR_MS, ShadowFamily, binary64_in, bounded_source_string, due_key, hash, instant,
    ordered_keys, rows, unsigned,
};
use crate::maintenance_canonical_json::{field, text};

const SOURCE_VERSION: &str = "adjustment-shadow-source-projection/v1";
const SOURCE_KEYS: &[&str] = &[
    "contractVersion",
    "family",
    "registrationSha256",
    "candidateSha256",
    "sourceSha256",
    "dueKey",
    "issuedAt",
    "rowCount",
    "rows",
];
const TEMPERATURE_SOURCE_KEYS: &[&str] = &[
    "contractVersion",
    "family",
    "registrationSha256",
    "candidateSha256",
    "sourceSha256",
    "dueKey",
    "issuedAt",
    "recentErrorState",
    "rowCount",
    "rows",
];
const RAIN_SOURCE_KEYS: &[&str] = &[
    "contractVersion",
    "family",
    "registrationSha256",
    "candidateSha256",
    "sourceSha256",
    "dueKey",
    "issuedAt",
    "causalInputs",
    "rowCount",
    "rows",
];
const TEMPERATURE_ROW_KEYS: &[&str] = &[
    "validAt",
    "leadHours",
    "modelLeadHours",
    "referenceAt",
    "receivedAt",
    "sourceSha256",
    "contentSha256",
    "adapterVersion",
    "dataset",
    "providerKey",
    "providerResponseSha256",
    "modelCycle",
    "upstreamModel",
    "rawTemperatureC64",
    "rawRelativeHumidityPercent64",
    "rawWindSpeedMps64",
    "bestMatchContentSha256",
    "bestMatchProductRunAt",
    "bestMatchSourceId",
    "bestMatchTemperatureC64",
];
const WIND_ROW_KEYS: &[&str] = &[
    "validAt",
    "leadHours",
    "modelLeadHours",
    "referenceAt",
    "receivedAt",
    "sourceSha256",
    "contentSha256",
    "adapterVersion",
    "contractEpoch",
    "dataset",
    "providerKey",
    "sourceKey",
    "sourceConfigFingerprint",
    "upstreamModel",
    "revisionCount",
    "windSpeedMps64",
    "windGustMps64",
];
const RAIN_ROW_KEYS: &[&str] = &[
    "validAt",
    "leadHours",
    "modelLeadHours",
    "referenceAt",
    "receivedAt",
    "sourceSha256",
    "contentSha256",
    "adapterVersion",
    "contractEpoch",
    "dataset",
    "providerKey",
    "sourceKey",
    "sourceConfigFingerprint",
    "upstreamModel",
    "revisionCount",
    "precipitationMm64",
];
const TEMPERATURE_RECENT_STATE_KEYS: &[&str] = &[
    "b24C",
    "b72C",
    "cohort",
    "localDates",
    "mad72C",
    "maximumSourceRunInitializedAt",
    "maximumSourceValidAt",
    "n24",
    "n72",
    "sourceKeys",
    "supported",
    "targetRunInitializedAt",
    "windowEndValidAt",
];

/// Validate one exact-order, already parsed private source projection.
pub(crate) fn validate_shadow_source(
    values: &impl ApiMaintenanceReceiptCanonicalValues,
    doc: &JsonValue,
    byte_len: usize,
) -> Option<()> {
    let family = ShadowFamily::read(doc)?;
    let top_keys = match family {
        ShadowFamily::Temperature => TEMPERATURE_SOURCE_KEYS,
        ShadowFamily::Wind => SOURCE_KEYS,
        ShadowFamily::Rain => RAIN_SOURCE_KEYS,
    };
    ordered_keys(doc, top_keys)?;
    // retain only the frozen source contract within its family byte budget
    if text(doc, "contractVersion")? != SOURCE_VERSION || byte_len > family.source_byte_cap() {
        return None;
    }
    hash(doc, "registrationSha256")?;
    hash(doc, "candidateSha256")?;
    let source_sha256 = hash(doc, "sourceSha256")?;
    let issued_ms = instant(values, doc, "issuedAt")?;
    due_key(values, doc)?;
    // validate the family-specific causal state before its source rows
    match family {
        ShadowFamily::Temperature => {
            validate_temperature_recent_error_state(
                values,
                field(doc, "recentErrorState")?,
                issued_ms,
            )?;
        }
        ShadowFamily::Rain => {
            validate_rain_causal_inputs(values, field(doc, "causalInputs")?, issued_ms)?;
        }
        ShadowFamily::Wind => {}
    }

    let source_rows = rows(doc, "rows")?;
    let declared_rows = unsigned(doc, "rowCount")?;
    let expected_rows = u64::try_from(family.row_count()).ok()?;
    // require the complete fixed family geometry before walking any source row
    if declared_rows != expected_rows || source_rows.len() != family.row_count() {
        return None;
    }
    let mut previous_valid_ms = None;
    // freeze exact source order, causal geometry, lineage and raw metrics
    for (index, row) in source_rows.iter().enumerate() {
        previous_valid_ms = Some(validate_source_row(
            values,
            row,
            family,
            index,
            source_sha256,
            issued_ms,
            previous_valid_ms,
        )?);
    }
    Some(())
}

/// Validate one source row and return its admitted valid-at epoch.
fn validate_source_row(
    values: &impl ApiMaintenanceReceiptCanonicalValues,
    row: &JsonValue,
    family: ShadowFamily,
    index: usize,
    source_sha256: &[u16],
    issued_ms: i64,
    previous_valid_ms: Option<i64>,
) -> Option<i64> {
    let row_keys = match family {
        ShadowFamily::Temperature => TEMPERATURE_ROW_KEYS,
        ShadowFamily::Wind => WIND_ROW_KEYS,
        ShadowFamily::Rain => RAIN_ROW_KEYS,
    };
    ordered_keys(row, row_keys)?;
    let valid_ms = instant(values, row, "validAt")?;
    let reference_ms = instant(values, row, "referenceAt")?;
    let received_ms = instant(values, row, "receivedAt")?;
    let row_source_sha256 = hash(row, "sourceSha256")?;
    hash(row, "contentSha256")?;
    // bind both stored row identities before admitting their metrics
    if row_source_sha256 != source_sha256 {
        return None;
    }
    let model_lead = unsigned(row, "modelLeadHours")?;
    let computed_model_lead = ceil_model_lead_hours(valid_ms, reference_ms)?;
    let expected_lead = u64::try_from(index).ok()?.checked_add(1)?;
    // reject late receipts, wrong model leads, future decisions and broken hourly order
    if received_ms > issued_ms
        || model_lead != computed_model_lead
        || computed_model_lead > 384
        || unsigned(row, "leadHours")? != expected_lead
        || valid_ms <= issued_ms
        || previous_valid_ms.is_some_and(|previous| valid_ms.checked_sub(previous) != Some(HOUR_MS))
    {
        return None;
    }

    let lineage_fields: &[&str] = match family {
        ShadowFamily::Temperature => &[
            "adapterVersion",
            "dataset",
            "providerKey",
            "modelCycle",
            "upstreamModel",
        ],
        ShadowFamily::Wind | ShadowFamily::Rain => &[
            "adapterVersion",
            "contractEpoch",
            "dataset",
            "providerKey",
            "sourceKey",
            "sourceConfigFingerprint",
            "upstreamModel",
        ],
    };
    // close every family lineage value to the Node UTF-16 string bound
    for key in lineage_fields {
        bounded_source_string(field(row, key)?)?;
    }
    // retain non-temperature source revisions as nonnegative safe integers
    if family != ShadowFamily::Temperature {
        unsigned(row, "revisionCount")?;
    }
    match family {
        ShadowFamily::Temperature => validate_temperature_row(values, row, issued_ms)?,
        ShadowFamily::Wind => {
            binary64_in(row, "windSpeedMps64", 0.0, 150.0, true)?;
            binary64_in(row, "windGustMps64", 0.0, 150.0, true)?;
        }
        ShadowFamily::Rain => {
            binary64_in(row, "precipitationMm64", 0.0, 2_000.0, true)?;
        }
    }
    Some(valid_ms)
}

/// Reproduce JavaScript Math.ceil for an exact millisecond lead difference.
fn ceil_model_lead_hours(valid_ms: i64, reference_ms: i64) -> Option<u64> {
    let difference = valid_ms.checked_sub(reference_ms)?;
    let quotient = difference.div_euclid(HOUR_MS);
    let remainder = u8::from(difference.rem_euclid(HOUR_MS) != 0);
    let ceiling = quotient.checked_add(i64::from(remainder))?;
    u64::try_from(ceiling).ok()
}

/// Validate the temperature-only comparator lineage and raw metrics.
fn validate_temperature_row(
    values: &impl ApiMaintenanceReceiptCanonicalValues,
    row: &JsonValue,
    issued_ms: i64,
) -> Option<()> {
    hash(row, "providerResponseSha256")?;
    hash(row, "bestMatchContentSha256")?;
    let best_match_ms = instant(values, row, "bestMatchProductRunAt")?;
    let best_match_source_id = field(row, "bestMatchSourceId")?;
    // keep the original positive decimal database identity and pre-decision run clock
    if !positive_decimal_source_id(best_match_source_id) || best_match_ms > issued_ms {
        return None;
    }
    binary64_in(row, "rawTemperatureC64", -100.0, 70.0, false)?;
    binary64_in(row, "rawRelativeHumidityPercent64", 0.0, 100.0, true)?;
    binary64_in(row, "rawWindSpeedMps64", 0.0, 150.0, true)?;
    binary64_in(row, "bestMatchTemperatureC64", -100.0, 70.0, true)?;
    Some(())
}

/// Validate the complete causal rolling-error state consumed by temperature inference.
fn validate_temperature_recent_error_state(
    values: &impl ApiMaintenanceReceiptCanonicalValues,
    state: &JsonValue,
    issued_ms: i64,
) -> Option<()> {
    ordered_keys(state, TEMPERATURE_RECENT_STATE_KEYS)?;
    instant(values, state, "targetRunInitializedAt")?;
    instant(values, state, "windowEndValidAt")?;
    // admit optional causal maxima only when they are normalized and not future
    for key in ["maximumSourceRunInitializedAt", "maximumSourceValidAt"] {
        match field(state, key)? {
            JsonValue::Null => {}
            _ => {
                if instant(values, state, key)? > issued_ms {
                    return None;
                }
            }
        }
    }
    // retain the frozen cohort, boolean support flag and bounded aggregate counts
    if text(state, "cohort")? != "ecmwf_single_run_hindcast"
        || !matches!(field(state, "supported")?, JsonValue::Boolean(_))
        || unsigned(state, "localDates")? > 366
        || unsigned(state, "n24")? > 10_000
        || unsigned(state, "n72")? > 10_000
    {
        return None;
    }
    let JsonValue::Array(source_keys) = field(state, "sourceKeys")? else {
        return None;
    };
    // preserve every bounded source key without admitting an unbounded state fan-in
    if source_keys.len() > 10_000 {
        return None;
    }
    for source_key in source_keys {
        bounded_source_string(source_key)?;
    }
    // optional fitted statistics remain decimal finite values rather than binary64 strings
    for key in ["b24C", "b72C", "mad72C"] {
        match field(state, key)? {
            JsonValue::Null => {}
            JsonValue::Number(value) if value.is_finite() => {}
            _ => return None,
        }
    }
    Some(())
}

/// Recognize the exact positive decimal source-id spelling with the Node UTF-16 cap.
fn positive_decimal_source_id(value: &JsonValue) -> bool {
    let JsonValue::String(value) = value else {
        return false;
    };
    matches!(value.first(), Some(first) if (u16::from(b'1')..=u16::from(b'9')).contains(first))
        && value.len() <= 20
        && value[1..]
            .iter()
            .all(|unit| (u16::from(b'0')..=u16::from(b'9')).contains(unit))
}
