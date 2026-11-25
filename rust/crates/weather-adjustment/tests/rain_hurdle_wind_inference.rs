use std::path::PathBuf;
use std::process::Command;

use weather_adjustment::{RainHurdleWindInferenceError, RainHurdleWindV1Model};
use weather_contracts::{
    Nullable, RainHurdleWindFeaturePerformance, RainHurdleWindPortableArtifactV1Heads,
    RainHurdleWindPortableArtifactV1HeadsNativeHead,
    RainHurdleWindPortableArtifactV1HeadsObjectiveEnum as RainHeadObjective,
    RainHurdleWindPortableArtifactV1Rules,
};

struct ModelTables {
    heads: RainHurdleWindPortableArtifactV1Heads,
    rules: Vec<RainHurdleWindPortableArtifactV1Rules>,
    category_scales: Vec<f64>,
}

/// Invoke the compiled TypeScript helper for one named parity fixture.
fn node_oracle(fixture: &str) -> String {
    let script =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/rain_hurdle_wind_oracle.mjs");
    let output = Command::new("node")
        .arg(script)
        .arg(fixture)
        .output()
        .expect("Node oracle must start");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("Node oracle must emit UTF-8")
}

/// Prove exact tree paths and category amounts against compiled production helpers.
#[test]
fn matches_compiled_rain_hurdle_wind_inference() {
    let tables = model_tables();
    let model = RainHurdleWindV1Model::new(&tables.heads, &tables.rules, &tables.category_scales)
        .expect("source-shaped tables must validate");
    // cover dry three wet categories explicit missing branches and float32 thresholds
    for fixture in [
        "dry",
        "category-1",
        "category-2",
        "category-3",
        "missing",
        "float32-threshold",
    ] {
        let prediction = model
            .predict(&features_for(fixture))
            .expect("normalized feature fixture must predict");
        assert_eq!(
            performance_bits(&prediction),
            node_oracle(fixture),
            "{fixture}"
        );
    }
}

/// Reject malformed model tables and feature vectors before traversal.
#[test]
fn rejects_invalid_rain_hurdle_wind_inputs() {
    let mut tables = model_tables();
    tables.rules[0].threshold = 0.2;
    assert_eq!(
        RainHurdleWindV1Model::new(&tables.heads, &tables.rules, &tables.category_scales)
            .expect_err("renamed threshold must fail"),
        RainHurdleWindInferenceError::InvalidRules,
    );

    let tables = model_tables();
    let model = RainHurdleWindV1Model::new(&tables.heads, &tables.rules, &tables.category_scales)
        .expect("source-shaped tables must validate");
    assert_eq!(
        model.predict(&[0.0; 106]),
        Err(RainHurdleWindInferenceError::InvalidFeatureVector),
    );
    let mut infinite = vec![0.0; 107];
    infinite[10] = f32::INFINITY;
    assert_eq!(
        model.predict(&infinite),
        Err(RainHurdleWindInferenceError::InvalidFeatureVector),
    );
    let mut missing_raw = vec![0.0; 107];
    missing_raw[5] = f32::NAN;
    assert_eq!(
        model.predict(&missing_raw),
        Err(RainHurdleWindInferenceError::InvalidRawAmount),
    );
}

/// Build the compact source-shaped tables used by both runtimes.
fn model_tables() -> ModelTables {
    ModelTables {
        heads: RainHurdleWindPortableArtifactV1Heads {
            atLeast0_1: split_head(0, 0.500_000_01, 1.0, -1.0, 1.0),
            atLeast1_0: split_head(1, 0.5, 0.5, -0.5, 0.0),
            atLeast2_5: split_head(2, 0.5, 0.25, -0.25, 1.0),
            amount: RainHurdleWindPortableArtifactV1HeadsNativeHead {
                objective: RainHeadObjective::RegGamma,
                baseScore: 2.0,
                trees: vec![[vec![0.0], vec![0.1], vec![-1.0], vec![-1.0], vec![0.0]]],
            },
        },
        rules: vec![
            RainHurdleWindPortableArtifactV1Rules {
                threshold: 0.1,
                cutoff: Nullable::Value(0.6),
            },
            RainHurdleWindPortableArtifactV1Rules {
                threshold: 1.0,
                cutoff: Nullable::Value(0.6),
            },
            RainHurdleWindPortableArtifactV1Rules {
                threshold: 2.5,
                cutoff: Nullable::Null,
            },
        ],
        category_scales: vec![0.8, 1.2, 2.0],
    }
}

/// Build one three-node binary split head.
fn split_head(
    feature_index: usize,
    threshold: f64,
    left_leaf: f64,
    right_leaf: f64,
    missing_left: f64,
) -> RainHurdleWindPortableArtifactV1HeadsNativeHead {
    RainHurdleWindPortableArtifactV1HeadsNativeHead {
        objective: RainHeadObjective::BinaryLogistic,
        baseScore: 0.5,
        trees: vec![[
            vec![feature_index as f64, 0.0, 0.0],
            vec![threshold, left_leaf, right_leaf],
            vec![1.0, -1.0, -1.0],
            vec![2.0, -1.0, -1.0],
            vec![missing_left, 0.0, 0.0],
        ]],
    }
}

/// Build one normalized 107-value feature vector by fixture name.
fn features_for(fixture: &str) -> Vec<f32> {
    let mut features = vec![0.0; 107];
    // select the same branch and raw category case as the Node fixture
    match fixture {
        "dry" => {
            features[0] = 1.0;
            features[1] = 1.0;
            features[2] = 1.0;
            features[5] = 0.0;
        }
        "category-1" => {
            features[0] = 0.0;
            features[1] = 1.0;
            features[2] = 1.0;
            features[5] = 0.2;
        }
        "category-2" => {
            features[0] = 0.0;
            features[1] = 0.0;
            features[2] = 1.0;
            features[5] = 0.2;
        }
        "category-3" => {
            features[0] = 1.0;
            features[1] = 1.0;
            features[2] = 1.0;
            features[5] = 3.0;
        }
        "missing" => {
            features[0] = f32::NAN;
            features[1] = f32::NAN;
            features[2] = f32::NAN;
            features[5] = 0.2;
        }
        "float32-threshold" => {
            features[0] = 0.5;
            features[1] = 1.0;
            features[2] = 1.0;
            features[5] = 0.2;
        }
        _ => panic!("unknown fixture {fixture}"),
    }
    features
}

/// Encode the five public prediction values as exact binary64 bits.
fn performance_bits(performance: &RainHurdleWindFeaturePerformance) -> String {
    [
        performance.correctedPrecipitationMm,
        performance.occurrenceProbabilityAtLeast0_1,
        performance.occurrenceProbabilityAtLeast1_0,
        performance.occurrenceProbabilityAtLeast2_5,
        performance.positiveAmountMm,
    ]
    .map(|value| format!("{:016x}", value.to_bits()))
    .join("\t")
}
