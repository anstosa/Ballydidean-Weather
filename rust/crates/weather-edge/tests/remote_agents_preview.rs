use std::path::PathBuf;
use std::process::Command;

use weather_edge::{
    AdmittedRemoteAgentsPreviewTarget, RemoteAgentsPreviewMode, RemoteAgentsPreviewResponse,
    handle_remote_agents_preview,
};

/// render one redirect using the source response field order.
fn signature(response: &RemoteAgentsPreviewResponse) -> String {
    format!(
        "{}|{}|{}|{}",
        response.status_code, response.cache_control, response.content_length, response.location
    )
}

/// return one exact value from the production Node oracle.
fn oracle_value<'a>(oracle: &'a str, prefix: &str) -> &'a str {
    oracle
        .lines()
        .find_map(|line| line.strip_prefix(prefix))
        .unwrap_or_else(|| panic!("missing oracle line: {prefix}"))
}

// verifies admitted normal desktop and mobile redirects against the production function
#[test]
fn matches_normal_remote_agents_preview_redirects() {
    let oracle = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/remote-agents-preview-node-oracle.mjs");
    let output = Command::new("node")
        .arg(oracle)
        .output()
        .expect("run RemoteAgents preview Node oracle");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let node = String::from_utf8(output.stdout).expect("Node oracle UTF-8");
    let cases = [
        (
            "desktop=",
            RemoteAgentsPreviewMode::Desktop,
            "/forecast?days=2#hour-1",
        ),
        (
            "mobile=",
            RemoteAgentsPreviewMode::Mobile,
            "/map/?layer=rain#now",
        ),
    ];

    // compare each already-normalized target without implementing a URI codec
    for (prefix, mode, location) in cases {
        let target = AdmittedRemoteAgentsPreviewTarget::admitted(mode, location);
        assert_eq!(target.mode(), mode);
        assert_eq!(target.location(), location);
        assert_eq!(
            signature(&handle_remote_agents_preview(target)),
            oracle_value(&node, prefix)
        );
    }
}
