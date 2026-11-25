use std::io::Write;
use std::process::{Command, Stdio};

use weather_edge::ArchiveWirePrimitives;

use super::*;
use crate::{
    forecast_scorecard_cli::tests::ordinary_family_card,
    maintenance_canonical_json::{field, text},
};

const HASH_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const HASH_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const HASH_C: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const HASH_D: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const HASH_E: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const HASH_F: &str = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";

const NODE_ORACLE: &str = r#"
import { readFileSync } from "node:fs";
import { buildAdjustmentMaintenanceScorecardV2 } from
  "./scripts/research/adjustment_maintenance_controller.mjs";
const input = JSON.parse(readFileSync(0, "utf8"));
const publication = buildAdjustmentMaintenanceScorecardV2(input);
process.stdout.write(JSON.stringify({
  bytes: publication.bytes.toString("base64"),
  scorecardSha256: publication.scorecardSha256,
}));
"#;

/// Match the controller's exact scorecard value, bytes, and SHA-256 identity.
#[test]
fn matches_source_scorecard_publication_material() {
    let wire = NativeWirePrimitives;
    let input = source_input();
    let attempt_sha256 = text(&input, "attemptSha256").expect("attempt identity");
    let generated_at = text(&input, "generatedAt").expect("generation instant");
    let operator_state = text(&input, "operatorState").expect("operator state");
    let source_revision = text(&input, "sourceRevision").expect("source revision");
    let publication = build_native_maintenance_scorecard_publication(
        NativeMaintenanceScorecardPublicationInput {
            attempt_sha256: &attempt_sha256,
            families: field(&input, "families").expect("families"),
            generated_at: &generated_at,
            identities: field(&input, "identities").expect("identities"),
            inputs: field(&input, "inputs").expect("inputs"),
            job: field(&input, "job").expect("job"),
            operator_state: &operator_state,
            progress: field(&input, "progress").expect("progress"),
            source_revision: &source_revision,
            warnings: field(&input, "warnings").expect("warnings"),
        },
    )
    .expect("build native scorecard publication");
    let mut child = Command::new("node")
        .args(["--input-type=module", "--eval", NODE_ORACLE])
        .current_dir(repository_root())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn scorecard source oracle");
    child
        .stdin
        .as_mut()
        .expect("oracle stdin")
        .write_all(
            &wire
                .encode_sorted_json(&input)
                .expect("encode source input"),
        )
        .expect("write source input");
    let output = child.wait_with_output().expect("wait for source oracle");
    assert!(
        output.status.success(),
        "source oracle failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let oracle = wire
        .decode_json(&output.stdout)
        .expect("decode oracle output");
    let oracle_bytes = wire
        .decode_standard_base64(&text(&oracle, "bytes").expect("oracle bytes"))
        .expect("decode oracle bytes");
    assert_eq!(publication.bytes, oracle_bytes);
    assert_eq!(
        publication.scorecard_sha256,
        text(&oracle, "scorecardSha256").expect("oracle digest")
    );
    assert_eq!(
        wire.encode_sorted_json(&publication.scorecard)
            .expect("encode projected scorecard"),
        publication.bytes
    );
}

/// Build the same complete genuine controller input used by the source regression.
fn source_input() -> JsonValue {
    let nullable = identities(JsonValue::Null);
    let raw = identities(string(HASH_B));
    object(vec![
        ("attemptSha256", string(HASH_A)),
        (
            "families",
            object(vec![
                ("rain", ordinary_family_card("rain")),
                ("temperature", ordinary_family_card("temperature")),
                ("wind", ordinary_family_card("wind")),
            ]),
        ),
        ("generatedAt", string("2026-10-10T12:00:00.000Z")),
        (
            "identities",
            object(vec![
                ("active", nullable.clone()),
                ("prior", nullable.clone()),
                ("raw", raw),
                ("shadow", nullable),
            ]),
        ),
        (
            "inputs",
            object(vec![
                ("adjustmentEvidenceManifestSha256", string(HASH_C)),
                ("adjustmentEvidenceWatermarkSha256", string(HASH_D)),
                ("forecastTrainingManifestSha256", string(HASH_E)),
                ("frontierSha256", string(HASH_F)),
                ("inputManifestSha256", string(HASH_A)),
                ("localDateFrom", string("2026-10-08")),
                ("localDateTo", string("2026-10-09")),
                ("reportSha256", string(HASH_B)),
                ("reportSha256s", identities(string(HASH_B))),
                ("sourceRevision", string(&"1".repeat(40))),
                ("targetCutoffAt", string("2026-10-10T07:00:00.000Z")),
            ]),
        ),
        (
            "job",
            object(vec![
                ("attemptState", string("blocked")),
                ("backlogState", string("blocked")),
                ("dueState", string("overdue")),
                ("successState", string("failed")),
            ]),
        ),
        ("operatorState", string("enabled")),
        (
            "progress",
            object(vec![
                ("confirmationCompletedEpochs", JsonValue::Number(0.0)),
                ("confirmationRequiredEpochs", JsonValue::Number(9.0)),
                ("rainCaptureExpiresAt", string("2027-10-08T07:00:00.000Z")),
                ("rollbackCompletedEpochs", JsonValue::Number(0.0)),
                ("rollbackRequiredEpochs", JsonValue::Number(0.0)),
            ]),
        ),
        ("sourceRevision", string(&"1".repeat(40))),
        (
            "warnings",
            object(vec![
                ("capacity", JsonValue::Array(Vec::new())),
                (
                    "capture",
                    JsonValue::Array(vec![string("semantic_catalog_unavailable")]),
                ),
                ("fallback", JsonValue::Array(Vec::new())),
                ("gauge", JsonValue::Array(Vec::new())),
                ("source", JsonValue::Array(Vec::new())),
            ]),
        ),
    ])
}

/// Project one family-keyed identity object in the source order.
fn identities(value: JsonValue) -> JsonValue {
    object(vec![
        ("rain", value.clone()),
        ("temperature", value.clone()),
        ("wind", value),
    ])
}

/// Construct one source-shaped JSON object without changing field values.
fn object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(key, value)| (key.encode_utf16().collect(), value))
            .collect(),
    )
}

/// Construct one exact generated JSON string.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Resolve the repository root from the runtime crate directory.
fn repository_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("workspace crate has repository root")
        .to_path_buf()
}
