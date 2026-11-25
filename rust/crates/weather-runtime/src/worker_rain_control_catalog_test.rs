use weather_contracts::{ForecastAdjustmentMaintenanceRuntimePackage, JsonValue};
use weather_database::Sha256Digest;

use super::{WorkerRainControlCatalog, select_worker_rain_control_catalog};
use crate::{ApiMaintenanceCandidateCodec, MaintenanceCanonicalJson};

const HASH: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const INSTALLED_AT: &str = "2026-09-01T00:00:00.000Z";

struct Values;

impl MaintenanceCanonicalJson for Values {
    // This selection fixture starts from an already decoded catalog.
    fn parse_canonical_object(&mut self, _bytes: &[u8]) -> Option<JsonValue> {
        None
    }

    // Selection never serializes a control receipt.
    fn encode_canonical_json(&mut self, _value: &JsonValue) -> Option<Vec<u8>> {
        None
    }

    // Selection never hashes a control receipt.
    fn sha256(&mut self, _bytes: &[u8]) -> Option<Sha256Digest> {
        None
    }
}

impl ApiMaintenanceCandidateCodec for Values {
    // Admit only the exact installed clock used by the fixture receipt.
    fn is_canonical_utc_instant(&mut self, value: &str) -> bool {
        value == INSTALLED_AT
    }

    // Control selection does not decode a model package.
    fn decode_maintenance_package(
        &mut self,
        _value: &JsonValue,
    ) -> Option<ForecastAdjustmentMaintenanceRuntimePackage> {
        None
    }
}

// Select the exact independent v3 control authority and retain its immutable links.
#[test]
fn selects_exact_v3_control_receipt() {
    let selected = select_worker_rain_control_catalog(&v3_catalog(), &mut Values)
        .expect("the closed v3 catalog should admit")
        .expect("the control slot should be selected");

    assert_eq!(
        selected,
        WorkerRainControlCatalog {
            action_sha256: HASH.to_owned(),
            control_state_sha256: HASH.to_owned(),
            ordinal_artifact_sha256: HASH.to_owned(),
            due_month: "2026-09".to_owned(),
            deployed_settings_sha256: HASH.to_owned(),
            fencing_token: "7".to_owned(),
            graph_manifest_sha256: HASH.to_owned(),
            source_member_root_sha256: HASH.to_owned(),
            source_receipt_root_sha256: HASH.to_owned(),
        }
    );
}

// Preserve the Node loader's closed fallback for catalogs predating control slots.
#[test]
fn older_catalog_has_no_independent_control_slot() {
    let catalog = object(vec![
        (
            "contractVersion",
            string("adjustment-installed-candidate-catalog/v2"),
        ),
        ("entries", JsonValue::Array(Vec::new())),
    ]);

    assert_eq!(
        select_worker_rain_control_catalog(&catalog, &mut Values)
            .expect("the bounded older catalog should remain valid"),
        None
    );
}

// Build the exact v3 rain control slot shape used by the installed loader.
fn v3_catalog() -> JsonValue {
    object(vec![
        (
            "contractVersion",
            string("adjustment-installed-candidate-catalog/v3"),
        ),
        (
            "entries",
            JsonValue::Array(vec![object(vec![
                ("family", string("rain")),
                ("receipt", receipt()),
                ("registration", JsonValue::Null),
                ("slot", string("control")),
            ])]),
        ),
    ])
}

// Build one closed root-installed control receipt with all graph identities present.
fn receipt() -> JsonValue {
    strings(&[
        ("actionKind", "control_reference"),
        ("actionSha256", HASH),
        (
            "contractVersion",
            "adjustment-installed-rain-control-reference-receipt/v1",
        ),
        ("controlPlaneSha256", HASH),
        ("controlStateSha256", HASH),
        ("custodyAnchorSha256", HASH),
        ("deployedCommit", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        (
            "deployedImageDigest",
            "sha256:1111111111111111111111111111111111111111111111111111111111111111",
        ),
        ("deployedRelease", "2026.09.01-1"),
        ("deployedSettingsSha256", HASH),
        ("dueMonth", "2026-09"),
        ("fencingToken", "7"),
        ("graphManifestSha256", HASH),
        ("installedAt", INSTALLED_AT),
        ("ordinalArtifactSha256", HASH),
        ("sourceMemberRootSha256", HASH),
        ("sourceReceiptRootSha256", HASH),
    ])
}

// Build one object without relying on a JSON encoder.
fn object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(key, value)| (key.encode_utf16().collect(), value))
            .collect(),
    )
}

// Build one all-string object in the source field order.
fn strings(fields: &[(&str, &str)]) -> JsonValue {
    object(
        fields
            .iter()
            .map(|(key, value)| (*key, string(value)))
            .collect(),
    )
}

// Build one exact UTF-16 JSON string.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}
