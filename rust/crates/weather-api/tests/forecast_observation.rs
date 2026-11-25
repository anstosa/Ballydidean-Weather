use std::path::PathBuf;
use std::process::Command;

// verifies the compiled Node private route baseline used by typed service tests
#[test]
fn matches_compiled_node_private_forecast_observation_route() {
    let oracle = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/forecast-observation-node-oracle.mjs");
    let output = Command::new("node")
        .arg(oracle)
        .output()
        .expect("run private forecast observation Node oracle");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8(output.stdout)
            .expect("Node oracle UTF-8")
            .contains("private forecast observation route Node oracle passed")
    );
}
