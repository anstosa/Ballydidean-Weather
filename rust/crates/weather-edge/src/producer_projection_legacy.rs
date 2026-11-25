//! Exact admission for the original revision projection producer contracts.

use std::collections::HashSet;

use weather_contracts::{AdjustmentRevisionProjectionKind, JsonValue};

use crate::archive_wire::{
    ArchiveWireCodec, ArchiveWirePrimitives, array, bool_value, exact_keys, field, hash, member,
    string, text,
};
use crate::private_revision_archive::{
    AdmittedPrivateRevisionProjection, PrivateRevisionProjectionDependency,
    PrivateRevisionProjectionShape,
};
use crate::producer_projection::{
    binary64, bounded_text, family_limit, instant_field, integer, nullable_binary64, ordered,
};
use crate::revision_archive_canonical::{RevisionArchiveCanonicalCodec, lower_hex};

const TOP_KEYS: &[&str] = &[
    "contractVersion",
    "family",
    "logicalKey",
    "logicalReceivedAt",
    "projectionKind",
    "rows",
    "source",
    "storedContentSha256",
];
const TEMPERATURE_NATIVE_TOP_KEYS: &[&str] = &[
    "contractVersion",
    "family",
    "logicalKey",
    "logicalReceivedAt",
    "projectionKind",
    "recentErrorState",
    "recentErrorStateSha256",
    "rows",
    "source",
    "storedContentSha256",
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
const SOURCE_KEYS: &[&str] = &[
    "adapterVersion",
    "contractEpoch",
    "dataset",
    "providerKey",
    "sourceConfigFingerprint",
    "sourceId",
    "sourceKey",
    "sourceKind",
    "upstreamModel",
];
const WEATHER_LOGICAL_KEYS: &[&str] = &["productRunAt", "sourceId", "sourceKind", "validAt"];
const NATIVE_LOGICAL_KEYS: &[&str] = &[
    "contentSha256",
    "leadHours",
    "providerResponseSha256",
    "runInitializedAt",
    "siteId",
    "sourceId",
    "sourceType",
    "validAt",
];
const RAIN_LOGICAL_KEYS: &[&str] = &["inputSha256", "modelSha256", "runInitializedAt"];
const WEATHER_ROW_KEYS: &[&str] = &[
    "apparentTemperatureC64",
    "blackGlobeTemperatureC64",
    "cloudCoverPercent64",
    "contentSha256",
    "pm25MicrogramsPerCubicMeter64",
    "precipitationMm64",
    "precipitationRateMmPerHour64",
    "pressureHpa64",
    "relativeHumidityPercent64",
    "soilElectricalConductivityMicrosiemensPerCm64",
    "soilMoisturePercent64",
    "solarRadiationWm264",
    "temperatureC64",
    "uvIndex64",
    "validAt",
    "waterLevelM64",
    "wetBulbGlobeTemperatureC64",
    "windDirectionDegrees64",
    "windGustMps64",
    "windSpeedMps64",
];
const ANCHOR_ROW_KEYS: &[&str] = &[
    "apparentTemperatureC64",
    "cloudCoverPercent64",
    "contentSha256",
    "leadHours",
    "precipitationMm64",
    "pressureHpa64",
    "relativeHumidityPercent64",
    "temperatureC64",
    "validAt",
    "windDirectionDegrees64",
    "windGustMps64",
    "windSpeedMps64",
];
const TEMPERATURE_ROW_KEYS: &[&str] = &[
    "bestMatchContentSha256",
    "bestMatchProductRunAt",
    "bestMatchSourceId",
    "bestMatchTemperatureC64",
    "contentSha256",
    "modelCycle",
    "modelLeadHours",
    "rawRelativeHumidityPercent64",
    "rawTemperatureC64",
    "rawWindSpeedMps64",
    "validAt",
];
const RAIN_GATE_ROW_KEYS: &[&str] = &[
    "applied",
    "correctedPrecipitationMm64",
    "modelLeadHours",
    "rawPrecipitationMm64",
    "reasonCode",
    "validAt",
];

/// Admit one original single, grouped, or temperature-state producer projection.
pub(crate) fn admit_legacy_projection<P: ArchiveWirePrimitives>(
    codec: &ArchiveWireCodec<P>,
    value: &JsonValue,
    bytes: &[u8],
) -> Option<AdmittedPrivateRevisionProjection> {
    // dispatch only the three original contract versions
    match text(value, "contractVersion")?.as_str() {
        "adjustment-revision-projection/v1" => admit_v1(codec, value, bytes),
        "adjustment-revision-batch-projection/v2" => admit_grouped(codec, value, bytes),
        "adjustment-temperature-native-source-projection/v2" => {
            admit_temperature_native(codec, value, bytes)
        }
        _ => None,
    }
}

/// Admit one original ordinary projection and preserve its producer order.
fn admit_v1<P: ArchiveWirePrimitives>(
    codec: &ArchiveWireCodec<P>,
    value: &JsonValue,
    bytes: &[u8],
) -> Option<AdmittedPrivateRevisionProjection> {
    let validated = validate_v1(codec, value)?;
    let family = text(value, "family")?;
    // apply the selected family's original byte ceiling
    (bytes.len() <= family_limit(&family)?).then_some(())?;
    let encoded = ordered_projection(value, TOP_KEYS, validated.logical_keys, validated.row_keys)?;
    // refuse every alternate object order or scalar spelling
    (codec.encode_ordered(&encoded)?.as_slice() == bytes).then_some(())?;
    Some(admitted(
        validated.kind,
        PrivateRevisionProjectionShape::Single,
    ))
}

/// Admit one original grouped weather projection and its bounded row geometry.
fn admit_grouped<P: ArchiveWirePrimitives>(
    codec: &ArchiveWireCodec<P>,
    value: &JsonValue,
    bytes: &[u8],
) -> Option<AdmittedPrivateRevisionProjection> {
    // retain the exact grouped envelope and discriminators
    exact_keys(value, TOP_KEYS).then_some(())?;
    (text(value, "contractVersion")? == "adjustment-revision-batch-projection/v2").then_some(())?;
    let kind = projection_kind(value)?;
    let family = text(value, "family")?;
    let valid_discriminator = matches!(
        (kind, family.as_str()),
        (AdjustmentRevisionProjectionKind::ActualBestMatch, "wind")
            | (AdjustmentRevisionProjectionKind::TargetRevision, "shared")
    );
    valid_discriminator.then_some(())?;
    (bytes.len() <= family_limit(&family)?).then_some(())?;
    instant_field(codec, value, "logicalReceivedAt")?;
    hash(value, "storedContentSha256")?;
    let logical_key = field(value, "logicalKey")?;
    let source = field(value, "source")?;
    exact_keys(logical_key, WEATHER_LOGICAL_KEYS).then_some(())?;
    exact_keys(source, SOURCE_KEYS).then_some(())?;
    validate_source(source, kind)?;
    validate_logical_key(codec, logical_key, kind)?;
    // bind the group header to its one source lineage
    (text(source, "sourceId")? == text(logical_key, "sourceId")?).then_some(())?;
    // preserve the comparator's fixed synthesized source names
    (kind != AdjustmentRevisionProjectionKind::ActualBestMatch
        || (text(source, "dataset")? == "best_match"
            && text(source, "upstreamModel")? == "best_match"))
        .then_some(())?;
    let rows = array(value, "rows")?;
    // retain one bounded complete causal group
    (!rows.is_empty() && rows.len() <= 168).then_some(())?;
    validate_grouped_rows(codec, rows, kind)?;
    let first = rows.first()?;
    // bind the stored identity and logical clock to the first row
    (text(first, "contentSha256")? == text(value, "storedContentSha256")?
        && text(first, "validAt")? == text(logical_key, "validAt")?)
    .then_some(())?;
    let encoded = ordered_projection(value, TOP_KEYS, WEATHER_LOGICAL_KEYS, WEATHER_ROW_KEYS)?;
    // refuse every alternate object order or scalar spelling
    (codec.encode_ordered(&encoded)?.as_slice() == bytes).then_some(())?;
    Some(admitted(
        kind,
        PrivateRevisionProjectionShape::Grouped {
            row_count: rows.len(),
        },
    ))
}

/// Admit one source-decision-frozen temperature run and its state digest.
fn admit_temperature_native<P: ArchiveWirePrimitives>(
    codec: &ArchiveWireCodec<P>,
    value: &JsonValue,
    bytes: &[u8],
) -> Option<AdmittedPrivateRevisionProjection> {
    // retain the additive envelope without weakening the ordinary projection
    exact_keys(value, TEMPERATURE_NATIVE_TOP_KEYS).then_some(())?;
    (text(value, "contractVersion")? == "adjustment-temperature-native-source-projection/v2"
        && text(value, "family")? == "temperature"
        && text(value, "projectionKind")? == "native_source")
        .then_some(())?;
    (bytes.len() <= family_limit("temperature")?).then_some(())?;
    let common = JsonValue::Object(
        TOP_KEYS
            .iter()
            .map(|key| {
                let selected = if *key == "contractVersion" {
                    string("adjustment-revision-projection/v1")
                } else {
                    field(value, key)?.clone()
                };
                Some(member(key, selected))
            })
            .collect::<Option<Vec<_>>>()?,
    );
    let validated = validate_v1(codec, &common)?;
    // require the additive contract to remain the complete temperature run
    (validated.kind == AdjustmentRevisionProjectionKind::NativeSource
        && validated.row_keys == TEMPERATURE_ROW_KEYS)
        .then_some(())?;
    let state = field(value, "recentErrorState")?;
    validate_recent_error_state(codec, value, state)?;
    let ordered_state = ordered(state, TEMPERATURE_RECENT_STATE_KEYS)?;
    let state_bytes = codec.encode_ordered(&ordered_state)?;
    let state_preimage = state_bytes.strip_suffix(b"\n")?;
    // bind the database's no-newline fixed-order state preimage
    (lower_hex(codec.sha256(state_preimage)) == text(value, "recentErrorStateSha256")?)
        .then_some(())?;
    let encoded = ordered_temperature_projection(value, &ordered_state)?;
    // refuse every alternate object order or scalar spelling
    (codec.encode_ordered(&encoded)?.as_slice() == bytes).then_some(())?;
    Some(admitted(
        AdjustmentRevisionProjectionKind::NativeSource,
        PrivateRevisionProjectionShape::Single,
    ))
}

/// Retain the row schema selected while validating an ordinary projection.
struct ValidatedV1 {
    kind: AdjustmentRevisionProjectionKind,
    logical_keys: &'static [&'static str],
    row_keys: &'static [&'static str],
}

/// Validate every ordinary projection field without imposing outer byte order.
fn validate_v1<P: ArchiveWirePrimitives>(
    codec: &ArchiveWireCodec<P>,
    value: &JsonValue,
) -> Option<ValidatedV1> {
    // retain the exact original envelope and version
    exact_keys(value, TOP_KEYS).then_some(())?;
    (text(value, "contractVersion")? == "adjustment-revision-projection/v1").then_some(())?;
    let kind = projection_kind(value)?;
    let family = text(value, "family")?;
    family_limit(&family)?;
    instant_field(codec, value, "logicalReceivedAt")?;
    hash(value, "storedContentSha256")?;
    let logical_key = field(value, "logicalKey")?;
    let logical_keys = logical_keys(kind);
    exact_keys(logical_key, logical_keys).then_some(())?;
    let source = field(value, "source")?;
    exact_keys(source, SOURCE_KEYS).then_some(())?;
    validate_source(source, kind)?;
    validate_logical_key(codec, logical_key, kind)?;
    let row_keys = row_keys(value, kind)?;
    let expected_rows = expected_row_count(value, kind)?;
    let rows = array(value, "rows")?;
    // require one complete immutable serving projection
    (rows.len() == expected_rows).then_some(())?;
    validate_v1_rows(codec, value, rows, kind, row_keys, expected_rows)?;
    validate_stored_binding(value, rows, kind)?;
    Some(ValidatedV1 {
        kind,
        logical_keys,
        row_keys,
    })
}

/// Validate the fixed source lineage without admitting provider-private fields.
fn validate_source(source: &JsonValue, kind: AdjustmentRevisionProjectionKind) -> Option<()> {
    let valid_values = SOURCE_KEYS.iter().all(|name| {
        // accept only the source's nullable bounded string grammar
        matches!(field(source, name), Some(JsonValue::Null))
            || field(source, name).is_some_and(bounded_text)
    });
    valid_values.then_some(())?;
    // rain gate input uses its logical hashes instead of a source row
    match kind {
        AdjustmentRevisionProjectionKind::RainGateInput => SOURCE_KEYS
            .iter()
            .all(|name| matches!(field(source, name), Some(JsonValue::Null)))
            .then_some(()),
        AdjustmentRevisionProjectionKind::TargetRevision => SOURCE_KEYS
            .iter()
            .all(|name| {
                // preserve the one intentional absent target field
                if *name == "upstreamModel" {
                    matches!(field(source, name), Some(JsonValue::Null))
                } else {
                    !matches!(field(source, name), Some(JsonValue::Null))
                }
            })
            .then_some(()),
        _ => SOURCE_KEYS
            .iter()
            .all(|name| !matches!(field(source, name), Some(JsonValue::Null)))
            .then_some(()),
    }
}

/// Validate one projection kind's exact database logical key.
fn validate_logical_key<P: ArchiveWirePrimitives>(
    codec: &ArchiveWireCodec<P>,
    key: &JsonValue,
    kind: AdjustmentRevisionProjectionKind,
) -> Option<()> {
    // select the closed key grammar for the relation
    match kind {
        AdjustmentRevisionProjectionKind::ActualBestMatch => {
            decimal(field(key, "sourceId")?).then_some(())?;
            instant_field(codec, key, "validAt")?;
            instant_field(codec, key, "productRunAt")?;
            (text(key, "sourceKind")? == "forecast").then_some(())
        }
        AdjustmentRevisionProjectionKind::TargetRevision => {
            decimal(field(key, "sourceId")?).then_some(())?;
            instant_field(codec, key, "validAt")?;
            (matches!(field(key, "productRunAt"), Some(JsonValue::Null))
                && text(key, "sourceKind")? == "physical_sensor")
                .then_some(())
        }
        AdjustmentRevisionProjectionKind::RainGateInput => {
            hash(key, "inputSha256")?;
            hash(key, "modelSha256")?;
            instant_field(codec, key, "runInitializedAt")?;
            Some(())
        }
        AdjustmentRevisionProjectionKind::NativeSource => validate_native_logical_key(codec, key),
    }
}

/// Validate one anchor or complete temperature-run logical key.
fn validate_native_logical_key<P: ArchiveWirePrimitives>(
    codec: &ArchiveWireCodec<P>,
    key: &JsonValue,
) -> Option<()> {
    // choose the only two original native source types
    match text(key, "sourceType")?.as_str() {
        "forecast_anchor" => {
            decimal(field(key, "sourceId")?).then_some(())?;
            instant_field(codec, key, "validAt")?;
            integer(field(key, "leadHours")?, 1, 384)?;
            [
                "contentSha256",
                "providerResponseSha256",
                "runInitializedAt",
                "siteId",
            ]
            .iter()
            .all(|name| matches!(field(key, name), Some(JsonValue::Null)))
            .then_some(())
        }
        "ecmwf_temperature_run" => {
            hash(key, "contentSha256")?;
            hash(key, "providerResponseSha256")?;
            instant_field(codec, key, "runInitializedAt")?;
            decimal(field(key, "siteId")?).then_some(())?;
            ["leadHours", "sourceId", "validAt"]
                .iter()
                .all(|name| matches!(field(key, name), Some(JsonValue::Null)))
                .then_some(())
        }
        _ => None,
    }
}

/// Validate all ordinary rows and their consecutive multirow clocks.
fn validate_v1_rows<P: ArchiveWirePrimitives>(
    codec: &ArchiveWireCodec<P>,
    projection: &JsonValue,
    rows: &[JsonValue],
    kind: AdjustmentRevisionProjectionKind,
    row_keys: &[&str],
    expected_rows: usize,
) -> Option<()> {
    let mut previous_valid_at = None;
    // retain row order while checking complete geometry
    for (index, row) in rows.iter().enumerate() {
        exact_keys(row, row_keys).then_some(())?;
        let valid_at = instant_field(codec, row, "validAt")?;
        // preserve hourly multirow geometry
        if let Some(previous) = previous_valid_at
            && expected_rows > 1
            && valid_at.checked_sub(previous) != Some(3_600_000)
        {
            return None;
        }
        previous_valid_at = Some(valid_at);
        validate_v1_row(codec, projection, row, kind, index)?;
    }
    Some(())
}

/// Validate one row against its exact database-backed projection class.
fn validate_v1_row<P: ArchiveWirePrimitives>(
    codec: &ArchiveWireCodec<P>,
    projection: &JsonValue,
    row: &JsonValue,
    kind: AdjustmentRevisionProjectionKind,
    index: usize,
) -> Option<()> {
    let logical_key = field(projection, "logicalKey")?;
    // select kind-specific value validation
    match kind {
        AdjustmentRevisionProjectionKind::RainGateInput => {
            integer(field(row, "modelLeadHours")?, 1, 168)?;
            bool_value(row, "applied")?;
            binary64(field(row, "rawPrecipitationMm64")?)?;
            nullable_binary64(field(row, "correctedPrecipitationMm64")?)?;
            // retain database null without inventing a label
            (matches!(field(row, "reasonCode"), Some(JsonValue::Null))
                || field(row, "reasonCode").is_some_and(bounded_text))
            .then_some(())
        }
        AdjustmentRevisionProjectionKind::NativeSource
            if text(logical_key, "sourceType")? == "ecmwf_temperature_run" =>
        {
            validate_temperature_row(codec, row, index)
        }
        _ => {
            hash(row, "contentSha256")?;
            // anchors retain one bounded database lead
            (kind != AdjustmentRevisionProjectionKind::NativeSource
                || integer(field(row, "leadHours")?, 1, 384).is_some())
            .then_some(())?;
            valid_row_metrics(row).then_some(())
        }
    }
}

/// Validate one complete-run temperature row and its comparator horizon.
fn validate_temperature_row<P: ArchiveWirePrimitives>(
    codec: &ArchiveWireCodec<P>,
    row: &JsonValue,
    index: usize,
) -> Option<()> {
    hash(row, "contentSha256")?;
    (integer(field(row, "modelLeadHours")?, 1, 18)? == i64::try_from(index).ok()? + 1)
        .then_some(())?;
    matches!(text(row, "modelCycle")?.as_str(), "49r1" | "50r1").then_some(())?;
    // retain comparators only for lead seven through eighteen
    if index < 6 {
        [
            "bestMatchContentSha256",
            "bestMatchProductRunAt",
            "bestMatchSourceId",
            "bestMatchTemperatureC64",
        ]
        .iter()
        .all(|name| matches!(field(row, name), Some(JsonValue::Null)))
        .then_some(())?;
    } else {
        hash(row, "bestMatchContentSha256")?;
        instant_field(codec, row, "bestMatchProductRunAt")?;
        decimal(field(row, "bestMatchSourceId")?).then_some(())?;
        nullable_binary64(field(row, "bestMatchTemperatureC64")?)?;
    }
    binary64(field(row, "rawTemperatureC64")?)?;
    nullable_binary64(field(row, "rawRelativeHumidityPercent64")?)?;
    nullable_binary64(field(row, "rawWindSpeedMps64")?)?;
    Some(())
}

/// Validate grouped weather rows and their kind-specific ordering.
fn validate_grouped_rows<P: ArchiveWirePrimitives>(
    codec: &ArchiveWireCodec<P>,
    rows: &[JsonValue],
    kind: AdjustmentRevisionProjectionKind,
) -> Option<()> {
    let mut previous_valid_at = None;
    // retain producer row order while enforcing the group geometry
    for row in rows {
        exact_keys(row, WEATHER_ROW_KEYS).then_some(())?;
        let valid_at = instant_field(codec, row, "validAt")?;
        hash(row, "contentSha256")?;
        // require strict target order and exact hourly comparator order
        if let Some(previous) = previous_valid_at {
            let elapsed = valid_at.checked_sub(previous)?;
            (elapsed > 0
                && (kind != AdjustmentRevisionProjectionKind::ActualBestMatch
                    || elapsed == 3_600_000))
                .then_some(())?;
        }
        previous_valid_at = Some(valid_at);
        valid_row_metrics(row).then_some(())?;
    }
    Some(())
}

/// Validate the original recent-error state and its run relationship.
fn validate_recent_error_state<P: ArchiveWirePrimitives>(
    codec: &ArchiveWireCodec<P>,
    projection: &JsonValue,
    state: &JsonValue,
) -> Option<()> {
    exact_keys(state, TEMPERATURE_RECENT_STATE_KEYS).then_some(())?;
    hash(projection, "recentErrorStateSha256")?;
    let target = instant_field(codec, state, "targetRunInitializedAt")?;
    let window_end = instant_field(codec, state, "windowEndValidAt")?;
    // validate each nullable retained source clock
    for name in ["maximumSourceRunInitializedAt", "maximumSourceValidAt"] {
        let value = field(state, name)?;
        // validate every present clock canonically
        if !matches!(value, JsonValue::Null) {
            instant_field(codec, state, name)?;
        }
    }
    let logical_key = field(projection, "logicalKey")?;
    (text(state, "cohort")? == "ecmwf_single_run_hindcast"
        && text(state, "targetRunInitializedAt")? == text(logical_key, "runInitializedAt")?
        && target.checked_sub(window_end) == Some(7 * 3_600_000))
    .then_some(())?;
    let n24 = integer(field(state, "n24")?, 0, 24)?;
    let n72 = integer(field(state, "n72")?, n24, 72)?;
    let local_dates = integer(field(state, "localDates")?, 0, n72)?;
    let source_keys = array(state, "sourceKeys")?;
    // bind all retained source keys to the seventy-two-hour sample count
    (usize::try_from(n72).ok()? == source_keys.len()
        && source_keys.iter().all(short_string)
        && source_keys
            .iter()
            .filter_map(string_units)
            .collect::<HashSet<_>>()
            .len()
            == source_keys.len())
    .then_some(())?;
    // retain the same redundant lower bound as the producer contract
    (local_dates >= 0).then_some(())?;
    ["b24C", "b72C", "mad72C"]
        .iter()
        .all(|name| bounded_nullable_statistic(field(state, name)))
        .then_some(())
}

/// Bind stored content to the first database row or run-level identity.
fn validate_stored_binding(
    projection: &JsonValue,
    rows: &[JsonValue],
    kind: AdjustmentRevisionProjectionKind,
) -> Option<()> {
    let first = rows.first()?;
    let logical_key = field(projection, "logicalKey")?;
    let stored = text(projection, "storedContentSha256")?;
    // choose the source contract's stored-content binding
    match kind {
        AdjustmentRevisionProjectionKind::ActualBestMatch
        | AdjustmentRevisionProjectionKind::TargetRevision => (text(first, "contentSha256")?
            == stored
            && text(first, "validAt")? == text(logical_key, "validAt")?)
        .then_some(()),
        AdjustmentRevisionProjectionKind::NativeSource
            if text(logical_key, "sourceType")? == "forecast_anchor" =>
        {
            (text(first, "contentSha256")? == stored
                && text(first, "validAt")? == text(logical_key, "validAt")?
                && integer(field(first, "leadHours")?, 1, 384)?
                    == integer(field(logical_key, "leadHours")?, 1, 384)?)
            .then_some(())
        }
        AdjustmentRevisionProjectionKind::NativeSource => {
            (text(logical_key, "contentSha256")? == stored).then_some(())
        }
        AdjustmentRevisionProjectionKind::RainGateInput => Some(()),
    }
}

/// Rebuild one original projection in its producer's nested object order.
fn ordered_projection(
    value: &JsonValue,
    top_keys: &[&str],
    logical_keys: &[&str],
    row_keys: &[&str],
) -> Option<JsonValue> {
    let rows = array(value, "rows")?;
    Some(JsonValue::Object(
        top_keys
            .iter()
            .map(|key| {
                let selected = match *key {
                    "logicalKey" => ordered(field(value, key)?, logical_keys)?,
                    "rows" => JsonValue::Array(
                        rows.iter()
                            .map(|row| ordered(row, row_keys))
                            .collect::<Option<Vec<_>>>()?,
                    ),
                    "source" => ordered(field(value, key)?, SOURCE_KEYS)?,
                    _ => field(value, key)?.clone(),
                };
                Some(member(key, selected))
            })
            .collect::<Option<Vec<_>>>()?,
    ))
}

/// Rebuild the additive temperature projection in its producer's nested order.
fn ordered_temperature_projection(
    value: &JsonValue,
    ordered_state: &JsonValue,
) -> Option<JsonValue> {
    let rows = array(value, "rows")?;
    Some(JsonValue::Object(
        TEMPERATURE_NATIVE_TOP_KEYS
            .iter()
            .map(|key| {
                let selected = match *key {
                    "logicalKey" => ordered(field(value, key)?, NATIVE_LOGICAL_KEYS)?,
                    "recentErrorState" => ordered_state.clone(),
                    "rows" => JsonValue::Array(
                        rows.iter()
                            .map(|row| ordered(row, TEMPERATURE_ROW_KEYS))
                            .collect::<Option<Vec<_>>>()?,
                    ),
                    "source" => ordered(field(value, key)?, SOURCE_KEYS)?,
                    _ => field(value, key)?.clone(),
                };
                Some(member(key, selected))
            })
            .collect::<Option<Vec<_>>>()?,
    ))
}

/// Map the original projection discriminator to its generated contract enum.
fn projection_kind(value: &JsonValue) -> Option<AdjustmentRevisionProjectionKind> {
    // retain only the generated four-value union
    match text(value, "projectionKind")?.as_str() {
        "actual_best_match" => Some(AdjustmentRevisionProjectionKind::ActualBestMatch),
        "native_source" => Some(AdjustmentRevisionProjectionKind::NativeSource),
        "rain_gate_input" => Some(AdjustmentRevisionProjectionKind::RainGateInput),
        "target_revision" => Some(AdjustmentRevisionProjectionKind::TargetRevision),
        _ => None,
    }
}

/// Select the exact ordered logical key fields for one projection kind.
fn logical_keys(kind: AdjustmentRevisionProjectionKind) -> &'static [&'static str] {
    // preserve the source's per-relation field order
    match kind {
        AdjustmentRevisionProjectionKind::ActualBestMatch
        | AdjustmentRevisionProjectionKind::TargetRevision => WEATHER_LOGICAL_KEYS,
        AdjustmentRevisionProjectionKind::NativeSource => NATIVE_LOGICAL_KEYS,
        AdjustmentRevisionProjectionKind::RainGateInput => RAIN_LOGICAL_KEYS,
    }
}

/// Select the exact value-row schema from the native source type.
fn row_keys(
    projection: &JsonValue,
    kind: AdjustmentRevisionProjectionKind,
) -> Option<&'static [&'static str]> {
    let logical_key = field(projection, "logicalKey")?;
    // preserve the source's kind and subtype selection
    match kind {
        AdjustmentRevisionProjectionKind::RainGateInput => Some(RAIN_GATE_ROW_KEYS),
        AdjustmentRevisionProjectionKind::NativeSource
            if text(logical_key, "sourceType")? == "ecmwf_temperature_run" =>
        {
            Some(TEMPERATURE_ROW_KEYS)
        }
        AdjustmentRevisionProjectionKind::NativeSource => Some(ANCHOR_ROW_KEYS),
        _ => Some(WEATHER_ROW_KEYS),
    }
}

/// Select the only legal complete ordinary row count.
fn expected_row_count(
    projection: &JsonValue,
    kind: AdjustmentRevisionProjectionKind,
) -> Option<usize> {
    let logical_key = field(projection, "logicalKey")?;
    // preserve the fixed native and rain widths
    match kind {
        AdjustmentRevisionProjectionKind::RainGateInput => Some(23),
        AdjustmentRevisionProjectionKind::NativeSource
            if text(logical_key, "sourceType")? == "ecmwf_temperature_run" =>
        {
            Some(18)
        }
        _ => Some(1),
    }
}

/// Validate every row member named by the frozen binary64 suffix.
fn valid_row_metrics(row: &JsonValue) -> bool {
    let JsonValue::Object(fields) = row else {
        return false;
    };
    fields.iter().all(|(name, value)| {
        // validate only the producer's binary64-suffixed members
        name.strip_suffix(&['6' as u16, '4' as u16])
            .is_none_or(|_| nullable_binary64(value).is_some())
    })
}

/// Admit one arbitrary-length positive decimal database identity.
fn decimal(value: &JsonValue) -> bool {
    let JsonValue::String(units) = value else {
        return false;
    };
    matches!(units.first(), Some(first) if (b'1' as u16..=b'9' as u16).contains(first))
        && units
            .iter()
            .skip(1)
            .all(|unit| (b'0' as u16..=b'9' as u16).contains(unit))
}

/// Admit one one-to-128-code-unit recent-error source key.
fn short_string(value: &JsonValue) -> bool {
    matches!(value, JsonValue::String(units) if !units.is_empty() && units.len() <= 128)
}

/// Borrow one string's exact UTF-16 units for set equality.
fn string_units(value: &JsonValue) -> Option<&Vec<u16>> {
    let JsonValue::String(units) = value else {
        return None;
    };
    Some(units)
}

/// Admit one nullable finite recent-error statistic within six degrees.
fn bounded_nullable_statistic(value: Option<&JsonValue>) -> bool {
    matches!(value, Some(JsonValue::Null))
        || matches!(value, Some(JsonValue::Number(number)) if number.is_finite() && number.abs() <= 6.0)
}

/// Construct archive metadata for one projection without dependencies.
fn admitted(
    projection_kind: AdjustmentRevisionProjectionKind,
    shape: PrivateRevisionProjectionShape,
) -> AdmittedPrivateRevisionProjection {
    AdmittedPrivateRevisionProjection {
        projection_kind,
        shape,
        dependency: PrivateRevisionProjectionDependency::None,
    }
}
