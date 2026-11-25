//! Compact generated-v2 coverage for portable rain artifact admission.

use weather_contracts::{
    JsonValue, Nullable, RainHurdleWindPortableArtifact, RainHurdleWindPortableArtifactV1Heads,
    RainHurdleWindPortableArtifactV1HeadsNativeHead,
    RainHurdleWindPortableArtifactV1HeadsObjectiveEnum, RainHurdleWindPortableArtifactV1Rules,
    RainHurdleWindPortableArtifactV2, RainHurdleWindPortableArtifactV2ContractVersionLiteral,
    RainHurdleWindProjectionId,
};
use weather_database::Sha256Digest;

use crate::rain_portable_artifact::admit_rain_portable_artifact;

const FEATURE_NAMES_SHA256: &str =
    "6c8be26782bae556a152aa83ecf2f9dec2089561a635a20d6510f16a979a3650";
const TREE_COUNT: usize = 160;
const FEATURE_COUNT: usize = 107;

/// Build one ordered dependency-free JSON object.
fn object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(name, value)| (name.encode_utf16().collect(), value))
            .collect(),
    )
}

/// Build one dependency-free JSON string.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Build the compact feature-name vector used by the admitted codec double.
fn feature_names() -> Vec<String> {
    (0..FEATURE_COUNT)
        .map(|index| format!("feature_{index}"))
        .collect()
}

/// Build the feature-name JSON value whose digest is owned by the injected codec.
fn feature_names_json() -> JsonValue {
    JsonValue::Array(
        feature_names()
            .into_iter()
            .map(|name| string(&name))
            .collect(),
    )
}

/// Build one valid single-leaf portable tree.
fn tree() -> [Vec<f64>; 5] {
    [vec![0.0], vec![0.0], vec![-1.0], vec![-1.0], vec![0.0]]
}

/// Build one exact 160-tree native head.
fn head(
    objective: RainHurdleWindPortableArtifactV1HeadsObjectiveEnum,
) -> RainHurdleWindPortableArtifactV1HeadsNativeHead {
    RainHurdleWindPortableArtifactV1HeadsNativeHead {
        objective,
        baseScore: 0.5,
        trees: (0..TREE_COUNT).map(|_| tree()).collect(),
    }
}

/// Build one viable generated v2 artifact with all four native heads.
fn artifact() -> RainHurdleWindPortableArtifact {
    RainHurdleWindPortableArtifact::V2(RainHurdleWindPortableArtifactV2 {
        modelMonth: "2026-11".to_owned(),
        featureNames: feature_names(),
        rules: vec![
            RainHurdleWindPortableArtifactV1Rules {
                threshold: 0.1,
                cutoff: Nullable::Value(0.5),
            },
            RainHurdleWindPortableArtifactV1Rules {
                threshold: 1.0,
                cutoff: Nullable::Value(0.5),
            },
            RainHurdleWindPortableArtifactV1Rules {
                threshold: 2.5,
                cutoff: Nullable::Value(0.5),
            },
        ],
        categoryScales: vec![1.0, 1.0, 1.0],
        heads: RainHurdleWindPortableArtifactV1Heads {
            atLeast0_1: head(RainHurdleWindPortableArtifactV1HeadsObjectiveEnum::BinaryLogistic),
            atLeast1_0: head(RainHurdleWindPortableArtifactV1HeadsObjectiveEnum::BinaryLogistic),
            atLeast2_5: head(RainHurdleWindPortableArtifactV1HeadsObjectiveEnum::BinaryLogistic),
            amount: head(RainHurdleWindPortableArtifactV1HeadsObjectiveEnum::RegGamma),
        },
        contractVersion: RainHurdleWindPortableArtifactV2ContractVersionLiteral,
        projectionId: RainHurdleWindProjectionId::R0ExactRefit,
    })
}

/// Build one JSON tree retaining the five parallel native arrays.
fn tree_json() -> JsonValue {
    JsonValue::Array(vec![
        JsonValue::Array(vec![JsonValue::Number(0.0)]),
        JsonValue::Array(vec![JsonValue::Number(0.0)]),
        JsonValue::Array(vec![JsonValue::Number(-1.0)]),
        JsonValue::Array(vec![JsonValue::Number(-1.0)]),
        JsonValue::Array(vec![JsonValue::Number(0.0)]),
    ])
}

/// Build one closed native-head JSON object.
fn head_json(objective: &str) -> JsonValue {
    object(vec![
        ("baseScore", JsonValue::Number(0.5)),
        ("objective", string(objective)),
        (
            "trees",
            JsonValue::Array((0..TREE_COUNT).map(|_| tree_json()).collect()),
        ),
    ])
}

/// Build one exact v2 JSON document without parsing or a custom codec.
fn document() -> JsonValue {
    object(vec![
        (
            "categoryScales",
            JsonValue::Array(vec![
                JsonValue::Number(1.0),
                JsonValue::Number(1.0),
                JsonValue::Number(1.0),
            ]),
        ),
        ("contractVersion", string("rain-hurdle-wind-runtime/v2")),
        ("featureNames", feature_names_json()),
        (
            "heads",
            object(vec![
                ("0.1", head_json("binary:logistic")),
                ("1.0", head_json("binary:logistic")),
                ("2.5", head_json("binary:logistic")),
                ("amount", head_json("reg:gamma")),
            ]),
        ),
        ("modelMonth", string("2026-11")),
        (
            "rules",
            JsonValue::Array(vec![
                object(vec![
                    ("cutoff", JsonValue::Number(0.5)),
                    ("threshold", JsonValue::Number(0.1)),
                ]),
                object(vec![
                    ("cutoff", JsonValue::Number(0.5)),
                    ("threshold", JsonValue::Number(1.0)),
                ]),
                object(vec![
                    ("cutoff", JsonValue::Number(0.5)),
                    ("threshold", JsonValue::Number(2.5)),
                ]),
            ]),
        ),
        ("projectionId", string("R0_exact_refit")),
    ])
}

/// Return the Node-expected digest only for the exact fixture feature vector.
fn feature_digest(value: &JsonValue) -> Option<Sha256Digest> {
    (value == &feature_names_json()).then_some(Sha256Digest::admitted(FEATURE_NAMES_SHA256))
}

#[test]
fn admits_one_closed_generated_v2_artifact_with_four_160_tree_heads() {
    let doc = document();
    let expected_doc = doc.clone();
    let decoded = artifact();
    let admitted = admit_rain_portable_artifact(
        &doc,
        move |value| (value == &expected_doc).then_some(decoded),
        feature_digest,
    );

    assert!(matches!(
        admitted,
        Some(RainHurdleWindPortableArtifact::V2(_))
    ));
}

#[test]
fn rejects_wrong_feature_digest_tree_count_and_tree_cell() {
    let doc = document();
    let decoded = artifact();
    assert!(
        admit_rain_portable_artifact(
            &doc,
            |_| Some(decoded.clone()),
            |_| Some(Sha256Digest::admitted("0".repeat(64))),
        )
        .is_none()
    );

    let mut short_artifact = decoded.clone();
    // retain the other three heads while shortening only the first
    match &mut short_artifact {
        RainHurdleWindPortableArtifact::V2(short) => {
            short.heads.atLeast0_1.trees.pop();
        }
        RainHurdleWindPortableArtifact::V1(_) => panic!("fixture must remain v2"),
    }
    assert!(admit_rain_portable_artifact(&doc, |_| Some(short_artifact), feature_digest).is_none());

    let mut bad_cell_artifact = decoded;
    // retain valid topology while moving one feature index out of domain
    match &mut bad_cell_artifact {
        RainHurdleWindPortableArtifact::V2(bad_cell) => {
            bad_cell.heads.amount.trees[0][0][0] = 107.0;
        }
        RainHurdleWindPortableArtifact::V1(_) => panic!("fixture must remain v2"),
    }
    assert!(
        admit_rain_portable_artifact(&doc, |_| Some(bad_cell_artifact), feature_digest).is_none()
    );
}

#[test]
fn rejects_an_extension_inside_the_closed_head_model() {
    let mut doc = document();
    let JsonValue::Object(fields) = &mut doc else {
        panic!("fixture document must be an object");
    };
    let heads = fields
        .iter_mut()
        .find_map(|(name, value)| {
            name.iter()
                .copied()
                .eq("heads".encode_utf16())
                .then_some(value)
        })
        .expect("fixture must retain heads");
    let JsonValue::Object(heads) = heads else {
        panic!("fixture heads must be an object");
    };
    let amount = heads
        .iter_mut()
        .find_map(|(name, value)| {
            name.iter()
                .copied()
                .eq("amount".encode_utf16())
                .then_some(value)
        })
        .expect("fixture must retain the amount head");
    let JsonValue::Object(amount) = amount else {
        panic!("fixture amount head must be an object");
    };
    amount.push(("extra".encode_utf16().collect(), JsonValue::Null));

    assert!(admit_rain_portable_artifact(&doc, |_| Some(artifact()), feature_digest).is_none());
}
