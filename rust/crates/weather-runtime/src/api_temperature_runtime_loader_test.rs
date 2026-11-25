//! Compare native temperature startup selection on checked-in model bytes with the original.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::OnceLock,
    time::{SystemTime, UNIX_EPOCH},
};

use weather_contracts::{
    FieldPresence, ForecastAdjustmentMaintenanceRuntimePackage,
    ForecastAdjustmentTemperatureCanaryRuntimeBundle, JsonValue,
    LoadedForecastAdjustmentTemperatureCanaryRuntime as Runtime,
    LoadedForecastAdjustmentTemperatureCanaryRuntimeV1 as LegacyRuntime,
    LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledReasonCodeEnum as Reason,
    MaintenanceShadowServingAuthority, Nullable, RainHurdleWindPortableArtifact,
};
use weather_database::Sha256Digest;

use crate::{
    ApiMaintenanceReceiptCanonicalValues, ApiMaintenanceShadowEvidenceValues,
    ApiStandardRuntimeValues, MaintenanceReceiptScalar, NativeForecastRuntimeFiles,
    api_wind_runtime_loader_test::frame,
    load_installed_temperature_runtime,
    maintenance_canonical_json::{field, text},
};

const FINITE: &str = "3e82073a266ca88c15f492f86bbefbca8b8cda029520af6cc78e0a0062ee50dd";
const PERMANENT: &str = "4d4e229b42823e53d2db062ec18c625bb2d2378a8a46d641fa95fabb59501b0e";

struct Material {
    document: JsonValue,
    bytes: Vec<u8>,
    sha256: String,
    base64: String,
}

struct Oracle {
    root: JsonValue,
    material: Vec<Material>,
}

impl Oracle {
    /// fetch one canonical member by its exact source document
    fn member(&self, predicate: impl Fn(&JsonValue) -> bool) -> &Material {
        self.material
            .iter()
            .find(|member| predicate(&member.document))
            .expect("recorded Node counterpart")
    }

    /// fetch an original finite or permanent bundle by its address
    fn bundle(&self, digest: &str) -> &Material {
        self.member(|node| text(node, "bundleSha256").as_deref() == Some(digest))
    }
}

impl ApiMaintenanceReceiptCanonicalValues for Oracle {
    /// resolve only the original canonical clocks carried by the fixture
    fn canonical_utc_epoch_milliseconds(&self, utc: &str) -> Option<i64> {
        let JsonValue::Number(value) = field(field(&self.root, "clocks")?, utc)? else {
            return None;
        };
        Some(*value as i64)
    }

    /// reuse the original canonical digests including authorization preimages
    fn sha256(&self, bytes: &[u8]) -> Option<Sha256Digest> {
        self.material
            .iter()
            .find(|member| member.bytes == bytes)
            .map(|member| Sha256Digest::admitted(member.sha256.as_str()))
    }

    /// legacy file selection never emits database receipt fields
    fn unsigned_receipt_sha256(
        &self,
        _: &[(&str, MaintenanceReceiptScalar<'_>)],
    ) -> Option<Sha256Digest> {
        None
    }
}

impl ApiMaintenanceShadowEvidenceValues for Oracle {
    /// supply the original JSON value for exactly one exported material member
    fn decode_shadow_json(&self, bytes: &[u8]) -> Option<JsonValue> {
        self.material
            .iter()
            .find(|member| member.bytes == bytes)
            .map(|member| member.document.clone())
    }

    /// ordered private bodies are outside this fixture
    fn encode_shadow_ordered_json(&self, _: &JsonValue) -> Option<Vec<u8>> {
        None
    }

    /// the independent qualified maintenance owner is not invoked here
    fn decode_shadow_comparator_base64(&self, _: &[u16]) -> Option<Vec<u8>> {
        None
    }

    /// use the original node canonical serialization for the verified value
    fn encode_shadow_sorted_json(&self, data: &JsonValue) -> Option<Vec<u8>> {
        self.material
            .iter()
            .find(|member| &member.document == data)
            .map(|member| member.bytes.clone())
    }

    /// the tested legacy branch does not substitute a package
    fn decode_shadow_maintenance_package(
        &self,
        _: &JsonValue,
    ) -> Option<ForecastAdjustmentMaintenanceRuntimePackage> {
        None
    }

    /// rain inference does not take part in legacy temperature selection
    fn decode_shadow_rain_artifact(&self, _: &JsonValue) -> Option<RainHurdleWindPortableArtifact> {
        None
    }

    /// the general wind model string policy is outside the temperature fixture
    fn check_legacy_wind_runtime_value(&self, _: &[u16]) -> Option<()> {
        None
    }
}

impl ApiStandardRuntimeValues for Oracle {
    /// use the verified canonical calendar labels of the actual fitted model
    fn standard_calendar_matches(&self, icu: &str, tzdata: &str) -> bool {
        let fingerprint = field(&self.bundle(PERMANENT).document, "runtimeFingerprint").unwrap();
        text(fingerprint, "icuVersion").as_deref() == Some(icu)
            && text(fingerprint, "tzdataVersion").as_deref() == Some(tzdata)
    }

    /// emit exactly the artifact and authorization bytes tested by the node loader
    fn encode_standard_base64(&self, bytes: &[u8]) -> Option<String> {
        self.material
            .iter()
            .find(|member| member.bytes == bytes)
            .map(|member| member.base64.clone())
    }
}

/// run the existing-runtime counterpart once for both actual-file tests
fn oracle() -> &'static Oracle {
    static ORACLE: OnceLock<Oracle> = OnceLock::new();
    ORACLE.get_or_init(|| {
        let script = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/temperature-runtime-loader-node-oracle.mjs");
        let run = Command::new("node")
            .arg(script)
            .output()
            .expect("run Node counterpart");
        assert!(
            run.status.success(),
            "{}",
            String::from_utf8_lossy(&run.stderr)
        );
        let stream = String::from_utf8(run.stdout).expect("original frame stream");
        let mut lines = stream.lines();
        let root = frame(&mut lines);
        assert!(lines.next().is_none());
        let Some(JsonValue::Array(material)) = field(&root, "materials") else {
            panic!("missing material")
        };
        let material = material
            .iter()
            .map(|member| Material {
                document: field(member, "value").unwrap().clone(),
                bytes: text(member, "bytes").unwrap().into_bytes(),
                sha256: text(member, "sha256").unwrap(),
                base64: text(member, "base64").unwrap(),
            })
            .collect();
        Oracle { root, material }
    })
}

/// compare the retained authority bytes rather than only their advertised addresses
fn assert_authority(value: &MaintenanceShadowServingAuthority, expected: &JsonValue) {
    assert_eq!(
        value.receiptBase64,
        text(expected, "receiptBase64").unwrap()
    );
    assert_eq!(
        value.receiptMemberSha256,
        text(expected, "receiptMemberSha256").unwrap()
    );
    assert_eq!(
        value.artifactBase64,
        text(expected, "artifactBase64").map_or(Nullable::Null, Nullable::Value)
    );
    assert_eq!(
        value.artifactMemberSha256,
        text(expected, "artifactMemberSha256").map_or(Nullable::Null, Nullable::Value)
    );
    assert_eq!(
        value.artifactIdentitySha256,
        text(expected, "artifactIdentitySha256").map_or(Nullable::Null, Nullable::Value)
    );
}

/// require the generated fail-raw disposition and no comparator
fn assert_disabled(runtime: Runtime, reason: Reason) {
    assert!(
        matches!(runtime, Runtime::Legacy { value: LegacyRuntime::Disabled(value), comparatorAuthority: FieldPresence::Missing } if value.reasonCode == reason)
    );
}

#[test]
fn loads_actual_permanent_temperature_runtime_from_installed_files() {
    let oracle = oracle();
    let material = oracle.bundle(PERMANENT);
    let authorization = field(&material.document, "authorization").unwrap();
    let source_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../config/forecast-adjustments")
        .canonicalize()
        .unwrap();
    let files = NativeForecastRuntimeFiles::new(source_root).unwrap();
    assert_disabled(
        load_installed_temperature_runtime(
            &files,
            oracle,
            None,
            &mut || panic!("killed temperature must not read time"),
            &mut || panic!("must not request qualification"),
        ),
        Reason::CanaryKilled,
    );
    let loaded = load_installed_temperature_runtime(
        &files,
        oracle,
        Some("0"),
        &mut || text(authorization, "activatedAt"),
        &mut || panic!("legacy registry has independent authority"),
    );
    let Runtime::Legacy {
        value: LegacyRuntime::Active(active),
        comparatorAuthority: FieldPresence::Value(authority),
    } = loaded
    else {
        panic!("expected active checked-in permanent temperature model: {loaded:?}");
    };
    let ForecastAdjustmentTemperatureCanaryRuntimeBundle::Permanent(bundle) = active.bundle else {
        panic!("checked-in bundle must be permanent");
    };
    assert_eq!(bundle.bundleSha256, PERMANENT);
    assert_eq!(
        bundle.authorization.activatedAt,
        text(authorization, "activatedAt").unwrap()
    );
    let model = field(&material.document, "model").unwrap();
    let Some(JsonValue::Array(expected)) = field(model, "directCoefficients") else {
        panic!("expected direct coefficients");
    };
    let Nullable::Value(actual) = bundle.model.directCoefficients else {
        panic!("native direct coefficients missing");
    };
    assert_eq!(actual.len(), expected.len());
    // preserve every fitted value rather than only verifying the descriptor
    for (actual, expected) in actual.iter().zip(expected) {
        assert_eq!(&JsonValue::Number(*actual), expected);
    }
    assert_authority(
        &authority,
        field(field(&oracle.root, "active").unwrap(), "authority").unwrap(),
    );
}

struct Temporary(PathBuf);
impl Drop for Temporary {
    /// remove the bounded local fixture after verification
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn binds_actual_finite_temperature_window_and_raw_registry_receipt() {
    let oracle = oracle();
    let bundle = oracle.bundle(FINITE);
    let authorization = field(&bundle.document, "authorization").unwrap();
    let registry = oracle.member(|node| {
        field(node, "activeBundle")
            .and_then(|active| text(active, "bundleSha256"))
            .as_deref()
            == Some(FINITE)
    });
    let raw = oracle.member(|node| text(node, "rawReason").as_deref() == Some("policy_raw"));
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temporary = Temporary(PathBuf::from(format!(
        "/var/tmp/weather-temp-startup-{}-{nonce}",
        std::process::id()
    )));
    let bundles = temporary.0.join("ballydidean/temperature-canary-bundles");
    fs::create_dir_all(&bundles).unwrap();
    let registry_path = temporary.0.join("ballydidean-temperature-canary.json");
    fs::write(&registry_path, &registry.bytes).unwrap();
    let files = NativeForecastRuntimeFiles::new(temporary.0.clone()).unwrap();
    assert_disabled(
        load_installed_temperature_runtime(
            &files,
            oracle,
            Some("0"),
            &mut || panic!("missing bundle must not read time"),
            &mut || panic!("legacy registry cannot qualify"),
        ),
        Reason::BundleMissing,
    );
    fs::write(bundles.join(format!("sha256-{FINITE}.json")), &bundle.bytes).unwrap();
    let active = load_installed_temperature_runtime(
        &files,
        oracle,
        Some("0"),
        &mut || text(authorization, "activatedAt"),
        &mut || panic!("legacy registry cannot qualify"),
    );
    let Runtime::Legacy {
        value: LegacyRuntime::Active(value),
        comparatorAuthority: FieldPresence::Value(authority),
    } = active
    else {
        panic!("expected finite activation: {active:?}");
    };
    assert!(
        matches!(value.bundle, ForecastAdjustmentTemperatureCanaryRuntimeBundle::TimeLimited(value) if value.bundleSha256 == FINITE)
    );
    assert_authority(
        &authority,
        field(&oracle.root, "finiteActiveAuthority").unwrap(),
    );
    assert_disabled(
        load_installed_temperature_runtime(
            &files,
            oracle,
            Some("0"),
            &mut || text(authorization, "expiresAt"),
            &mut || panic!("legacy registry cannot qualify"),
        ),
        Reason::CanaryExpired,
    );
    fs::write(&registry_path, &raw.bytes).unwrap();
    let raw = load_installed_temperature_runtime(
        &files,
        oracle,
        Some("0"),
        &mut || panic!("raw selection must not read time"),
        &mut || panic!("raw registry cannot qualify"),
    );
    let Runtime::MaintenanceDisabled(raw) = raw else {
        panic!("expected original policy-raw selection");
    };
    assert_authority(
        &raw.comparatorAuthority,
        field(&oracle.root, "rawAuthority").unwrap(),
    );
}
