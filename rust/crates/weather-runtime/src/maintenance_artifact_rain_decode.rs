//! Decode the generated portable rain model DTO from the shared dependency-free JSON tree.

use weather_contracts::{
    JsonValue, Nullable, RainHurdleWindPortableArtifact, RainHurdleWindPortableArtifactV1,
    RainHurdleWindPortableArtifactV1ContractVersionLiteral, RainHurdleWindPortableArtifactV1Heads,
    RainHurdleWindPortableArtifactV1HeadsNativeHead,
    RainHurdleWindPortableArtifactV1HeadsObjectiveEnum, RainHurdleWindPortableArtifactV1Rules,
    RainHurdleWindPortableArtifactV2, RainHurdleWindPortableArtifactV2ContractVersionLiteral,
    RainHurdleWindProjectionId,
};

use crate::maintenance_canonical_json::{exact_keys, field, text};

const V1_VERSION: &str = RainHurdleWindPortableArtifactV1ContractVersionLiteral::WIRE_VALUE;
const V2_VERSION: &str = RainHurdleWindPortableArtifactV2ContractVersionLiteral::WIRE_VALUE;
const V1_KEYS: &[&str] = &[
    "categoryScales",
    "contractVersion",
    "featureNames",
    "heads",
    "modelMonth",
    "rules",
];
const V2_KEYS: &[&str] = &[
    "categoryScales",
    "contractVersion",
    "featureNames",
    "heads",
    "modelMonth",
    "projectionId",
    "rules",
];
const RULE_KEYS: &[&str] = &["cutoff", "threshold"];
const HEADS_KEYS: &[&str] = &["0.1", "1.0", "2.5", "amount"];
const HEAD_KEYS: &[&str] = &["baseScore", "objective", "trees"];

/// Fields shared by the two closed portable artifact versions.
struct DecodedPortableFields {
    model_month: String,
    feature_names: Vec<String>,
    rules: Vec<RainHurdleWindPortableArtifactV1Rules>,
    category_scales: Vec<f64>,
    heads: RainHurdleWindPortableArtifactV1Heads,
}

/// Decode only the generated DTO shape; the existing API owner validates model semantics.
pub(crate) fn decode_rain_portable_artifact(
    document: &JsonValue,
) -> Option<RainHurdleWindPortableArtifact> {
    let version = text(document, "contractVersion")?;
    match version.as_str() {
        V1_VERSION => {
            exact_keys(document, V1_KEYS).then_some(())?;
            let fields = decode_shared_fields(document)?;
            Some(RainHurdleWindPortableArtifact::V1(
                RainHurdleWindPortableArtifactV1 {
                    contractVersion: RainHurdleWindPortableArtifactV1ContractVersionLiteral,
                    modelMonth: fields.model_month,
                    featureNames: fields.feature_names,
                    rules: fields.rules,
                    categoryScales: fields.category_scales,
                    heads: fields.heads,
                },
            ))
        }
        V2_VERSION => {
            exact_keys(document, V2_KEYS).then_some(())?;
            let fields = decode_shared_fields(document)?;
            Some(RainHurdleWindPortableArtifact::V2(
                RainHurdleWindPortableArtifactV2 {
                    modelMonth: fields.model_month,
                    featureNames: fields.feature_names,
                    rules: fields.rules,
                    categoryScales: fields.category_scales,
                    heads: fields.heads,
                    contractVersion: RainHurdleWindPortableArtifactV2ContractVersionLiteral,
                    projectionId: projection_id(&text(document, "projectionId")?)?,
                },
            ))
        }
        _ => None,
    }
}

/// Decode the common artifact fields without applying inference-model constraints twice.
fn decode_shared_fields(document: &JsonValue) -> Option<DecodedPortableFields> {
    Some(DecodedPortableFields {
        model_month: text(document, "modelMonth")?,
        feature_names: string_array(field(document, "featureNames")?)?,
        rules: rules(field(document, "rules")?)?,
        category_scales: number_array(field(document, "categoryScales")?)?,
        heads: heads(field(document, "heads")?)?,
    })
}

/// Decode the ordered public feature-name vector without normalizing UTF-16 strings.
fn string_array(value: &JsonValue) -> Option<Vec<String>> {
    let JsonValue::Array(values) = value else {
        return None;
    };
    values
        .iter()
        .map(|value| {
            let JsonValue::String(value) = value else {
                return None;
            };
            String::from_utf16(value).ok()
        })
        .collect()
}

/// Decode one runtime-number vector while leaving finite/range checks to model admission.
fn number_array(value: &JsonValue) -> Option<Vec<f64>> {
    let JsonValue::Array(values) = value else {
        return None;
    };
    values
        .iter()
        .map(|value| match value {
            JsonValue::Number(value) => Some(*value),
            _ => None,
        })
        .collect()
}

/// Decode the public threshold rules with their explicit nullable cutoff.
fn rules(value: &JsonValue) -> Option<Vec<RainHurdleWindPortableArtifactV1Rules>> {
    let JsonValue::Array(values) = value else {
        return None;
    };
    values
        .iter()
        .map(|value| {
            exact_keys(value, RULE_KEYS).then_some(())?;
            Some(RainHurdleWindPortableArtifactV1Rules {
                threshold: number(field(value, "threshold")?)?,
                cutoff: nullable_number(field(value, "cutoff")?)?,
            })
        })
        .collect()
}

/// Decode the four exact public model heads.
fn heads(value: &JsonValue) -> Option<RainHurdleWindPortableArtifactV1Heads> {
    exact_keys(value, HEADS_KEYS).then_some(())?;
    Some(RainHurdleWindPortableArtifactV1Heads {
        atLeast0_1: head(field(value, "0.1")?)?,
        atLeast1_0: head(field(value, "1.0")?)?,
        atLeast2_5: head(field(value, "2.5")?)?,
        amount: head(field(value, "amount")?)?,
    })
}

/// Decode one native head and its ordered five-array tree representation.
fn head(value: &JsonValue) -> Option<RainHurdleWindPortableArtifactV1HeadsNativeHead> {
    exact_keys(value, HEAD_KEYS).then_some(())?;
    let JsonValue::Array(trees) = field(value, "trees")? else {
        return None;
    };
    Some(RainHurdleWindPortableArtifactV1HeadsNativeHead {
        objective: objective(&text(value, "objective")?)?,
        baseScore: number(field(value, "baseScore")?)?,
        trees: trees.iter().map(tree).collect::<Option<_>>()?,
    })
}

/// Decode one tree only when all five parallel arrays are present.
fn tree(value: &JsonValue) -> Option<[Vec<f64>; 5]> {
    let JsonValue::Array(values) = value else {
        return None;
    };
    // avoid one temporary vector allocation per tree across the four heads
    let [features, thresholds, left, right, missing_left] = values.as_slice() else {
        return None;
    };
    Some([
        number_array(features)?,
        number_array(thresholds)?,
        number_array(left)?,
        number_array(right)?,
        number_array(missing_left)?,
    ])
}

/// Decode one number without adding model-specific range policy.
fn number(value: &JsonValue) -> Option<f64> {
    match value {
        JsonValue::Number(value) => Some(*value),
        _ => None,
    }
}

/// Preserve JSON null separately from a decoded numeric cutoff.
fn nullable_number(value: &JsonValue) -> Option<Nullable<f64>> {
    match value {
        JsonValue::Null => Some(Nullable::Null),
        JsonValue::Number(value) => Some(Nullable::Value(*value)),
        _ => None,
    }
}

/// Decode the two exact native model objectives.
fn objective(value: &str) -> Option<RainHurdleWindPortableArtifactV1HeadsObjectiveEnum> {
    RainHurdleWindPortableArtifactV1HeadsObjectiveEnum::WIRE_VALUES
        .iter()
        .find_map(|(wire, decoded)| (*wire == value).then_some(*decoded))
}

/// Decode one additive projection identifier through the generated wire table.
fn projection_id(value: &str) -> Option<RainHurdleWindProjectionId> {
    RainHurdleWindProjectionId::WIRE_VALUES
        .iter()
        .find_map(|(wire, decoded)| (*wire == value).then_some(*decoded))
}
