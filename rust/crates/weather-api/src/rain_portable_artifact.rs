//! Admission for one decoded portable rain hurdle-wind artifact.

use weather_adjustment::RainHurdleWindV1Model;
use weather_contracts::{
    JsonValue, RainHurdleWindPortableArtifact, RainHurdleWindPortableArtifactV1,
    RainHurdleWindPortableArtifactV1Heads, RainHurdleWindPortableArtifactV2,
};

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
const HEAD_KEYS: &[&str] = &["0.1", "1.0", "2.5", "amount"];
const NATIVE_HEAD_KEYS: &[&str] = &["baseScore", "objective", "trees"];
const V1_VERSION: &str = "rain-hurdle-wind-runtime/v1";
const V2_VERSION: &str = "rain-hurdle-wind-runtime/v2";
const FEATURE_NAMES_SHA256: &str =
    "6c8be26782bae556a152aa83ecf2f9dec2089561a635a20d6510f16a979a3650";
const FEATURE_COUNT: usize = 107;
const TREES_PER_HEAD: usize = 160;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PortableArtifactVersion {
    V1,
    V2,
}

/// Admit one closed decoded portable artifact after its raw bytes and JSON were verified.
pub fn admit_rain_portable_artifact(
    doc: &JsonValue,
    decode: impl FnOnce(&JsonValue) -> Option<RainHurdleWindPortableArtifact>,
    feature_names_digest: impl FnOnce(&JsonValue) -> Option<weather_database::Sha256Digest>,
) -> Option<RainHurdleWindPortableArtifact> {
    let version = portable_artifact_version(doc)?;
    let fields = object(doc)?;
    let expected_keys = match version {
        PortableArtifactVersion::V1 => V1_KEYS,
        PortableArtifactVersion::V2 => V2_KEYS,
    };
    exact_unordered_keys(fields, expected_keys)?;
    validate_raw_feature_names(doc, feature_names_digest)?;
    validate_closed_numerical_documents(doc)?;

    let artifact = decode(doc)?;
    // bind the decoder's closed DTO variant back to the original root version
    match (&artifact, version) {
        (RainHurdleWindPortableArtifact::V1(_), PortableArtifactVersion::V1)
        | (RainHurdleWindPortableArtifact::V2(_), PortableArtifactVersion::V2) => {}
        _ => return None,
    }
    validate_typed_artifact(&artifact)?;
    Some(artifact)
}

/// Read one exact portable artifact version from the original document.
fn portable_artifact_version(doc: &JsonValue) -> Option<PortableArtifactVersion> {
    let value = field(doc, "contractVersion")?;
    // accept only the two public portable runtime contracts
    if utf16_equals_ascii(value, V1_VERSION) {
        Some(PortableArtifactVersion::V1)
    } else if utf16_equals_ascii(value, V2_VERSION) {
        Some(PortableArtifactVersion::V2)
    } else {
        None
    }
}

/// Bind the exact feature array geometry and canonical compact JSON digest.
fn validate_raw_feature_names(
    doc: &JsonValue,
    feature_names_digest: impl FnOnce(&JsonValue) -> Option<weather_database::Sha256Digest>,
) -> Option<()> {
    let feature_names = field(doc, "featureNames")?;
    let JsonValue::Array(names) = feature_names else {
        return None;
    };
    // retain the frozen feature geometry before hashing its compact JSON representation
    if names.len() != FEATURE_COUNT
        || names
            .iter()
            .any(|name| !matches!(name, JsonValue::String(_)))
    {
        return None;
    }
    (feature_names_digest(feature_names)?.as_str() == FEATURE_NAMES_SHA256).then_some(())
}

/// Close the rule, head, and tree container shapes on the original document.
fn validate_closed_numerical_documents(doc: &JsonValue) -> Option<()> {
    let JsonValue::Array(rules) = field(doc, "rules")? else {
        return None;
    };
    // close every rule before the typed numerical model validates its values
    if rules.iter().any(|rule| {
        object(rule)
            .and_then(|fields| exact_unordered_keys(fields, RULE_KEYS))
            .is_none()
    }) {
        return None;
    }

    let heads = object(field(doc, "heads")?)?;
    exact_unordered_keys(heads, HEAD_KEYS)?;
    // close all four named heads and require the frozen training tree count
    if HEAD_KEYS.iter().any(|name| {
        let Some(head) = field_from_object(heads, name).and_then(object) else {
            return true;
        };
        if exact_unordered_keys(head, NATIVE_HEAD_KEYS).is_none() {
            return true;
        }
        !matches!(field_from_object(head, "trees"), Some(JsonValue::Array(trees)) if trees.len() == TREES_PER_HEAD)
    }) {
        return None;
    }
    Some(())
}

/// Reuse the inference model's shared numerical rule and tree invariants.
fn validate_typed_artifact(artifact: &RainHurdleWindPortableArtifact) -> Option<()> {
    match artifact {
        RainHurdleWindPortableArtifact::V1(artifact) => validate_typed_v1(artifact),
        RainHurdleWindPortableArtifact::V2(artifact) => validate_typed_v2(artifact),
    }
}

/// Validate one decoded v1 portable artifact.
fn validate_typed_v1(artifact: &RainHurdleWindPortableArtifactV1) -> Option<()> {
    validate_model_month(&artifact.modelMonth)?;
    validate_typed_model(
        &artifact.heads,
        &artifact.rules,
        &artifact.categoryScales,
        artifact.featureNames.len(),
    )
}

/// Validate one decoded v2 portable artifact.
fn validate_typed_v2(artifact: &RainHurdleWindPortableArtifactV2) -> Option<()> {
    validate_model_month(&artifact.modelMonth)?;
    validate_typed_model(
        &artifact.heads,
        &artifact.rules,
        &artifact.categoryScales,
        artifact.featureNames.len(),
    )
}

/// Validate the typed feature geometry and shared numerical model.
fn validate_typed_model(
    heads: &RainHurdleWindPortableArtifactV1Heads,
    rules: &[weather_contracts::RainHurdleWindPortableArtifactV1Rules],
    category_scales: &[f64],
    feature_count: usize,
) -> Option<()> {
    let exact_tree_counts = heads.atLeast0_1.trees.len() == TREES_PER_HEAD
        && heads.atLeast1_0.trees.len() == TREES_PER_HEAD
        && heads.atLeast2_5.trees.len() == TREES_PER_HEAD
        && heads.amount.trees.len() == TREES_PER_HEAD;
    // keep the returned typed DTO bound to the frozen feature and tree geometry
    if feature_count != FEATURE_COUNT || !exact_tree_counts {
        return None;
    }
    RainHurdleWindV1Model::new(heads, rules, category_scales).ok()?;
    Some(())
}

/// Admit an exact four-digit year and two-digit calendar month.
fn validate_model_month(value: &str) -> Option<()> {
    let bytes = value.as_bytes();
    // preserve the source regular expression without accepting wider numeric templates
    if bytes.len() != 7
        || !bytes[..4].iter().all(u8::is_ascii_digit)
        || bytes[4] != b'-'
        || !bytes[5..].iter().all(u8::is_ascii_digit)
        || !(b'0'..=b'1').contains(&bytes[5])
        || (bytes[5] == b'0' && !(b'1'..=b'9').contains(&bytes[6]))
        || (bytes[5] == b'1' && !(b'0'..=b'2').contains(&bytes[6]))
    {
        return None;
    }
    Some(())
}

/// Borrow one JSON object.
fn object(value: &JsonValue) -> Option<&[(Vec<u16>, JsonValue)]> {
    let JsonValue::Object(fields) = value else {
        return None;
    };
    Some(fields)
}

/// Find one ASCII-named field without allocating a UTF-8 key.
fn field<'a>(value: &'a JsonValue, name: &str) -> Option<&'a JsonValue> {
    field_from_object(object(value)?, name)
}

/// Find one ASCII-named member of an already admitted object.
fn field_from_object<'a>(fields: &'a [(Vec<u16>, JsonValue)], name: &str) -> Option<&'a JsonValue> {
    fields
        .iter()
        .find_map(|(key, value)| utf16_key_equals_ascii(key, name).then_some(value))
}

/// Require one unordered object to contain each expected key exactly once.
fn exact_unordered_keys(fields: &[(Vec<u16>, JsonValue)], expected: &[&str]) -> Option<()> {
    // reject extensions, omissions, and duplicate key spellings
    (fields.len() == expected.len()
        && expected.iter().all(|name| {
            fields
                .iter()
                .filter(|(key, _)| utf16_key_equals_ascii(key, name))
                .count()
                == 1
        }))
    .then_some(())
}

/// Compare one JSON string value with an ASCII literal.
fn utf16_equals_ascii(value: &JsonValue, expected: &str) -> bool {
    matches!(value, JsonValue::String(units) if utf16_key_equals_ascii(units, expected))
}

/// Compare UTF-16 code units with one ASCII field or literal.
fn utf16_key_equals_ascii(units: &[u16], expected: &str) -> bool {
    units.len() == expected.len()
        && units
            .iter()
            .copied()
            .zip(expected.bytes())
            .all(|(unit, byte)| unit == u16::from(byte))
}
