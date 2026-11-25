use weather_contracts::{
    DevelopmentLosoFoldResultV1, DevelopmentLosoFoldResultV1ContractVersionLiteral,
    DevelopmentLosoFoldResultV1FoldEnum, ForecastAdjustmentCoefficient,
    ForecastAdjustmentCoefficientDaypartEnum, ForecastAdjustmentCoefficientLevelEnum,
    ForecastAdjustmentCoefficientSeasonEnum, ForecastAdjustmentMetric,
    ForecastAdjustmentMetricBand, ForecastLeadBandKey, ForecastObservationProviderFamily,
    ForecastObservationStationKey, JsonValue, Nullable,
};
use weather_worker::wind_maintenance_development::WindAuxiliaryModel;

/// Project one station-excluded auxiliary model in its original object order.
pub(crate) fn auxiliary_model(model: &WindAuxiliaryModel<'_>) -> JsonValue {
    object([
        ("coefficients", coefficients(model.coefficients)),
        ("fold", fold(model.fold)),
        ("metricBand", metric_band(model.metric_band)),
        (
            "physicalStationKey",
            wire_text(
                model.physical_station_key,
                ForecastObservationStationKey::WIRE_VALUES,
            ),
        ),
    ])
}

/// Project one frozen development LOSO fold result without reordering evidence arrays.
pub(crate) fn development_fold(fold_result: &DevelopmentLosoFoldResultV1) -> JsonValue {
    object([
        (
            "auxiliaryModelSha256s",
            text_array(&fold_result.auxiliaryModelSha256s),
        ),
        (
            "bootstrapLowerBound",
            JsonValue::Number(fold_result.bootstrapLowerBound),
        ),
        (
            "contractVersion",
            text(DevelopmentLosoFoldResultV1ContractVersionLiteral::WIRE_VALUE),
        ),
        ("fold", fold(fold_result.fold)),
        (
            "materialHarmSliceKeys",
            text_array(&fold_result.materialHarmSliceKeys),
        ),
        ("metricBand", metric_band(&fold_result.metricBand)),
        (
            "nonnegativeStationFraction",
            JsonValue::Number(fold_result.nonnegativeStationFraction),
        ),
        ("passed", JsonValue::Boolean(fold_result.passed)),
        (
            "providerBalancedAdjustedLoss",
            JsonValue::Number(fold_result.providerBalancedAdjustedLoss),
        ),
        (
            "providerBalancedRawLoss",
            JsonValue::Number(fold_result.providerBalancedRawLoss),
        ),
        (
            "providerBalancedSkill",
            JsonValue::Number(fold_result.providerBalancedSkill),
        ),
        (
            "providerFamilies",
            JsonValue::Array(
                fold_result
                    .providerFamilies
                    .iter()
                    .copied()
                    .map(|family| wire_text(family, ForecastObservationProviderFamily::WIRE_VALUES))
                    .collect(),
            ),
        ),
        (
            "scoreableStationKeys",
            JsonValue::Array(
                fold_result
                    .scoreableStationKeys
                    .iter()
                    .copied()
                    .map(|station| wire_text(station, ForecastObservationStationKey::WIRE_VALUES))
                    .collect(),
            ),
        ),
    ])
}

/// Project one metric-band pair in its generated wire field order.
pub(crate) fn metric_band(band: &ForecastAdjustmentMetricBand) -> JsonValue {
    object([
        (
            "leadBand",
            wire_text(band.leadBand, ForecastLeadBandKey::WIRE_VALUES),
        ),
        (
            "metric",
            wire_text(band.metric, ForecastAdjustmentMetric::WIRE_VALUES),
        ),
    ])
}

/// Project a fitted coefficient array without changing hierarchy emission order.
pub(crate) fn coefficients(values: &[ForecastAdjustmentCoefficient]) -> JsonValue {
    JsonValue::Array(values.iter().map(coefficient).collect())
}

/// Construct one ordered object from borrowed UTF-8 field names.
pub(crate) fn object<const N: usize>(fields: [(&str, JsonValue); N]) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(key, value)| (key.encode_utf16().collect(), value))
            .collect(),
    )
}

/// Construct one JSON string without introducing a serializer boundary.
pub(crate) fn text(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Project one generated string enum through its canonical wire table.
pub(crate) fn wire_text<T: Copy + PartialEq>(
    value: T,
    wire_values: &[(&'static str, T)],
) -> JsonValue {
    let wire = wire_values
        .iter()
        .find_map(|(wire, candidate)| (*candidate == value).then_some(*wire))
        .expect("generated wire value must be exhaustive");
    text(wire)
}

/// Project one generated unsigned enum through its canonical wire table.
pub(crate) fn wire_unsigned<T: Copy + PartialEq>(value: T, wire_values: &[(u64, T)]) -> JsonValue {
    let wire = wire_values
        .iter()
        .find_map(|(wire, candidate)| (*candidate == value).then_some(*wire))
        .expect("generated wire value must be exhaustive");
    JsonValue::Number(wire as f64)
}

/// Project one fitted hierarchy coefficient in the original interface order.
pub(crate) fn coefficient(value: &ForecastAdjustmentCoefficient) -> JsonValue {
    object([
        ("coefficient", JsonValue::Number(value.coefficient)),
        (
            "daypart",
            nullable_wire_text(
                &value.daypart,
                ForecastAdjustmentCoefficientDaypartEnum::WIRE_VALUES,
            ),
        ),
        (
            "effectiveEventCount",
            JsonValue::Number(value.effectiveEventCount),
        ),
        (
            "leadBand",
            wire_text(value.leadBand, ForecastLeadBandKey::WIRE_VALUES),
        ),
        (
            "level",
            wire_unsigned(
                value.level,
                ForecastAdjustmentCoefficientLevelEnum::WIRE_VALUES,
            ),
        ),
        (
            "metric",
            wire_text(value.metric, ForecastAdjustmentMetric::WIRE_VALUES),
        ),
        ("month", nullable_unsigned(&value.month)),
        (
            "season",
            nullable_wire_text(
                &value.season,
                ForecastAdjustmentCoefficientSeasonEnum::WIRE_VALUES,
            ),
        ),
    ])
}

/// Project one generated fold number.
fn fold(value: DevelopmentLosoFoldResultV1FoldEnum) -> JsonValue {
    wire_unsigned(value, DevelopmentLosoFoldResultV1FoldEnum::WIRE_VALUES)
}

/// Project one nullable generated string enum.
fn nullable_wire_text<T: Copy + PartialEq>(
    value: &Nullable<T>,
    wire_values: &[(&'static str, T)],
) -> JsonValue {
    // preserve explicit JSON null rather than omitting the field
    match value {
        Nullable::Null => JsonValue::Null,
        Nullable::Value(value) => wire_text(*value, wire_values),
    }
}

/// Project one nullable safe unsigned integer.
fn nullable_unsigned(value: &Nullable<u64>) -> JsonValue {
    // preserve explicit JSON null rather than omitting the field
    match value {
        Nullable::Null => JsonValue::Null,
        Nullable::Value(value) => JsonValue::Number(*value as f64),
    }
}

/// Project an ordered string array without sorting or cloning UTF-8 material.
fn text_array(values: &[String]) -> JsonValue {
    JsonValue::Array(values.iter().map(|value| text(value)).collect())
}
