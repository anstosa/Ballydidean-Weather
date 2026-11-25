//! Positive capture epoch examples from the already compiled TypeScript verifier.

use std::path::Path;
use std::process::Command;

use weather_api::{CanonicalInstant, CurrentProjectionError, CurrentTimeResolver};
use weather_contracts::{JsDateOrString, JsonValue};
use weather_database::Sha256Digest;
use weather_runtime::MaintenanceCanonicalJson;

pub const FIXTURE_EPOCH: &str = "2026-10-08T16:00:00.000Z";

const NODE_ORACLE: &str = r#"
import { createHash } from "node:crypto";
import {
  canonicalJsonBytes,
  verifyAdjustmentRevisionCaptureEpochWitness,
} from "./packages/forecast-adjustment/dist/index.js";

const cases = [
  ["14", "1".repeat(64), "c683c4f937c7f02b00f6ab49f75268eead81d2a221a8a9a23e38f9e4802a11b0"],
  ["14", "1".repeat(64), "6de5c8c7efaa448aeb12bf1a9debe6fe7d4d4d1003ee0e21ab619ffa624c3424"],
  ["17", "6ea5cf5b770904bdfa557b29af2dd8899701d8f1b4a15b632f55ab5121032cbb",
    "107ce430f1ac9478d54d94646da7fa4a54cbe7112db13efd7244d8894d0defdf"],
  ["17", "6ea5cf5b770904bdfa557b29af2dd8899701d8f1b4a15b632f55ab5121032cbb",
    "3aec6846d009a2bb2cee02279dceb9bd2d3572dc1b3a83eed2ce9a1724311345"],
];
for (const [controlPlaneVersion, controlPlaneSha256, databaseMigrationHistorySha256] of cases) {
  const unsigned = {
    activationKind: "inert_v14_pre_activation",
    archiveCommitOrdinal: "0",
    catalogFrontierSha256: "5d932b9623819be9432877a513194158708719497d755155f5ed9a4b501b48af",
    contractVersion: "adjustment-revision-capture-epoch-witness/v1",
    controlPlaneSha256,
    controlPlaneVersion,
    databaseMigrationHistorySha256,
    epochAt: "2026-10-08T16:00:00.000Z",
    servingSnapshotSha256: "2".repeat(64),
    sourceCommit: "3".repeat(40),
    sourceRelease: "2026.10.08-1",
    sourceServerImageDigest: "sha256:" + "4".repeat(64),
    sourceWebImageDigest: "sha256:" + "5".repeat(64),
  };
  const preimage = canonicalJsonBytes(unsigned);
  const witnessSha256 = createHash("sha256").update(preimage).digest("hex");
  const bytes = canonicalJsonBytes({ ...unsigned, witnessSha256 });
  const witness = verifyAdjustmentRevisionCaptureEpochWitness(Buffer.from(bytes));
  console.log([bytes.slice(0, -1), preimage.slice(0, -1), witnessSha256, Date.parse(witness.epochAt)].join("\t"));
  // export fields as fixture data, not as a second JSON parser
  for (const [key, value] of Object.entries(witness).sort(([a], [b]) => a < b ? -1 : a > b ? 1 : 0)) {
    console.log(key + "\t" + value);
  }
  console.log("end");
}
"#;

#[derive(Clone)]
pub struct NodeCaptureEpoch {
    pub bytes: Vec<u8>,
    pub digest: String,
    pub clock: CanonicalInstant,
    unsigned_bytes: Vec<u8>,
    document: JsonValue,
    unsigned: JsonValue,
}

/// Obtain actual original bytes, self-hash and admitted positive tuples without parsing JSON in Rust.
pub fn node_capture_epoch_cases() -> Vec<NodeCaptureEpoch> {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("workspace root");
    let output = Command::new("node")
        .args(["--input-type=module", "-e", NODE_ORACLE])
        .current_dir(workspace)
        .output()
        .expect("run the compiled Node oracle");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).expect("fixture UTF-8");
    text.split_terminator("end\n")
        .map(|case| {
            let mut lines = case.lines();
            let header = lines.next().expect("fixture header");
            let header: [&str; 4] = header
                .split('\t')
                .collect::<Vec<_>>()
                .try_into()
                .expect("fixture header columns");
            let mut fields: Vec<(Vec<u16>, JsonValue)> = Vec::new();
            // retain the exact UTF-16 scalars exported by the original verifier
            for line in lines {
                let (key, value) = line.split_once('\t').expect("fixture field");
                fields.push((
                    key.encode_utf16().collect(),
                    JsonValue::String(value.encode_utf16().collect()),
                ));
            }
            let unsigned = JsonValue::Object(
                fields
                    .iter()
                    .filter(|(key, _)| !key.iter().copied().eq("witnessSha256".encode_utf16()))
                    .cloned()
                    .collect(),
            );
            NodeCaptureEpoch {
                bytes: format!("{}\n", header[0]).into_bytes(),
                unsigned_bytes: format!("{}\n", header[1]).into_bytes(),
                digest: header[2].to_owned(),
                clock: CanonicalInstant {
                    utc: FIXTURE_EPOCH.to_owned(),
                    epoch_milliseconds: header[3].parse().expect("fixture milliseconds"),
                },
                document: JsonValue::Object(fields),
                unsigned,
            }
        })
        .collect()
}

impl MaintenanceCanonicalJson for NodeCaptureEpoch {
    /// Expose the verified original decoded document from the fixture codec.
    fn parse_canonical_object(&mut self, bytes: &[u8]) -> Option<JsonValue> {
        (bytes == self.bytes).then(|| self.document.clone())
    }

    /// Return the original bytes only for exact equality with the Node material.
    fn encode_canonical_json(&mut self, value: &JsonValue) -> Option<Vec<u8>> {
        match value {
            value if value == &self.document => Some(self.bytes.clone()),
            value if value == &self.unsigned => Some(self.unsigned_bytes.clone()),
            _ => None,
        }
    }

    /// Admit only the observed original unsigned bytes to the fixture hash owner.
    fn sha256(&mut self, bytes: &[u8]) -> Option<Sha256Digest> {
        (bytes == self.unsigned_bytes).then(|| Sha256Digest::admitted(self.digest.clone()))
    }
}

impl CurrentTimeResolver for NodeCaptureEpoch {
    /// Use the exact original Date value for the one fixture clock.
    fn resolve(&self, value: &JsDateOrString) -> Result<CanonicalInstant, CurrentProjectionError> {
        let expected = JsDateOrString::String(self.clock.utc.encode_utf16().collect());
        (value == &expected)
            .then(|| self.clock.clone())
            .ok_or(CurrentProjectionError {
                code: "fixture.clock",
                detail: "unexpected witness clock".to_owned(),
            })
    }
}
