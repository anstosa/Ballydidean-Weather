//! Exact retained-incumbent parity against the deployed TypeScript artifact.

use std::{path::PathBuf, process::Command};

use weather_adjustment::RainHurdleWindV1Model;
use weather_contracts::RainHurdleWindFeaturePerformance;

/// compare the generated retained tables to the original deployed evaluator
#[test]
fn retained_incumbent_matches_deployed_v1_artifact_for_full_width_rows() {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/rain_retained_incumbent_node_oracle.mjs");
    let output = Command::new("node")
        .arg(script)
        .output()
        .expect("deployed rain incumbent oracle must start");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("oracle rows must be UTF-8");
    let model = RainHurdleWindV1Model::retained();

    // compare every Node-owned float32 row and exact binary64 result
    for line in stdout.lines() {
        let fields = line.split('\t').collect::<Vec<_>>();
        assert_eq!(fields.len(), 7, "oracle field count for {line}");
        let name = fields[0];
        let feature_bits = fields[1];
        let features = decode_features(feature_bits);
        let expected = fields[2..].join("\t");
        let prediction = model
            .predict(&features)
            .unwrap_or_else(|error| panic!("retained model rejected {name}: {error:?}"));
        let actual = performance_bits(&prediction);
        assert_eq!(
            actual, expected,
            "case={name} raw={} input_bits={feature_bits} node={expected} rust={actual}",
            features[5]
        );
    }
    assert_eq!(stdout.lines().count(), 7);
}

/// decode the oracle's complete original-width float32 input row
fn decode_features(encoded: &str) -> Vec<f32> {
    let features = encoded
        .split(',')
        .map(|bits| {
            f32::from_bits(u32::from_str_radix(bits, 16).expect("feature bits must be hexadecimal"))
        })
        .collect::<Vec<_>>();
    assert_eq!(features.len(), 107, "retained feature width");
    features
}

/// encode the five public prediction values as exact binary64 bits
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
