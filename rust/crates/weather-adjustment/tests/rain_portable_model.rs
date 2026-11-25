//! Exact portable-model parity against the compiled TypeScript scorer.

use std::{path::PathBuf, process::Command};

use weather_adjustment::{RainHurdleWindInferenceError, RainHurdleWindPortableModel};
use weather_contracts::{
    Nullable, RainHurdleWindPortableArtifact, RainHurdleWindPortableArtifactV1Heads,
    RainHurdleWindPortableArtifactV1HeadsNativeHead,
    RainHurdleWindPortableArtifactV1HeadsObjectiveEnum as Objective,
    RainHurdleWindPortableArtifactV1Rules, RainHurdleWindPortableArtifactV2,
    RainHurdleWindPortableArtifactV2ContractVersionLiteral, RainHurdleWindProjectionId,
};

/// Build the same numerical native head as the Node fixture.
fn head(base_score: f64, objective: Objective) -> RainHurdleWindPortableArtifactV1HeadsNativeHead {
    RainHurdleWindPortableArtifactV1HeadsNativeHead {
        baseScore: base_score,
        objective,
        trees: vec![[vec![0.0], vec![0.0], vec![-1.0], vec![-1.0], vec![0.0]]; 160],
    }
}

/// Retain identical head weights, thresholds and category scales on both sides.
fn artifact(projection: RainHurdleWindProjectionId, raw: bool) -> RainHurdleWindPortableArtifact {
    RainHurdleWindPortableArtifact::V2(RainHurdleWindPortableArtifactV2 {
        contractVersion: RainHurdleWindPortableArtifactV2ContractVersionLiteral,
        featureNames: vec!["retained feature".to_owned(); 107],
        modelMonth: "2026-03".to_owned(),
        projectionId: projection,
        categoryScales: vec![1.0; 3],
        rules: [0.1, 1.0, 2.5]
            .into_iter()
            .zip([0.34, 0.81, 0.81])
            .map(
                |(threshold, cutoff)| RainHurdleWindPortableArtifactV1Rules {
                    threshold,
                    cutoff: if raw {
                        Nullable::Null
                    } else {
                        Nullable::Value(cutoff)
                    },
                },
            )
            .collect(),
        heads: RainHurdleWindPortableArtifactV1Heads {
            atLeast0_1: head(0.30, Objective::BinaryLogistic),
            atLeast1_0: head(0.80, Objective::BinaryLogistic),
            atLeast2_5: head(0.75, Objective::BinaryLogistic),
            amount: head(0.75, Objective::RegGamma),
        },
    })
}

/// Compare every binary64 result with the original evaluator at every supplied row.
#[test]
fn matches_all_seven_portable_rain_arms_and_calendar_edges() {
    let script =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/rain_portable_model_node_oracle.mjs");
    let output = Command::new("node")
        .arg(script)
        .output()
        .expect("Node rain oracle starts");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = String::from_utf8(output.stdout).expect("oracle artifact rows are UTF-8");
    assert_eq!(output.lines().count(), 56);
    // compare every retained arm for the same model tables and feature bits
    for row in output.lines() {
        let row: Vec<&str> = row.split('\t').collect();
        let arm = row[0].parse::<usize>().unwrap();
        let (_, projection) = RainHurdleWindProjectionId::WIRE_VALUES[arm];
        let artifact = artifact(projection, row[1] == "raw");
        let model = RainHurdleWindPortableModel::new(&artifact).expect("portable v2 model");
        let mut features = [0.0f32; 107];
        features[5] = row[2].parse::<f32>().unwrap();
        let month = row[3].parse::<u8>().unwrap();
        let prediction = model
            .predict(&features, Some(month))
            .expect("scored v2 input");
        let actual = [
            prediction.correctedPrecipitationMm,
            prediction.occurrenceProbabilityAtLeast0_1,
            prediction.occurrenceProbabilityAtLeast1_0,
            prediction.occurrenceProbabilityAtLeast2_5,
            prediction.positiveAmountMm,
        ]
        .map(f64::to_bits);
        let expected: Vec<u64> = row[4..]
            .iter()
            .map(|hex| u64::from_str_radix(hex, 16).unwrap())
            .collect();
        assert_eq!(
            actual.as_slice(),
            expected.as_slice(),
            "{projection:?}, month {month}, raw {}",
            features[5]
        );
        assert_eq!(
            model.predict(&features, None),
            Err(RainHurdleWindInferenceError::InvalidLocalMonth)
        );
        assert_eq!(
            model.predict_with_local_month(&features[..106], || panic!(
                "geometry rejected before calendar"
            )),
            Err(RainHurdleWindInferenceError::InvalidFeatureVector)
        );
    }
}
