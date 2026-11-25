//! Differential coverage for catalog-qualified wind and temperature package serving.

use std::{cell::Cell, path::Path, process::Command, rc::Rc, sync::OnceLock};

use weather_contracts::*;
use weather_database::Sha256Digest;

use crate::{
    ApiMaintenanceCandidateCodec, ApiMaintenanceCandidateError, ApiMaintenanceCandidateFiles,
    ApiMaintenanceCandidateMembers, ApiMaintenanceReceiptCanonicalValues,
    ApiMaintenanceShadowEvidenceValues, ApiQualifiedCanaryFiles, ApiQualifiedCanaryMembers,
    ApiStandardRuntimeValues, InstalledQualifiedCanaryLoaders, MaintenanceCanonicalJson,
    MaintenanceReceiptScalar, NativeInstalledQualifiedCanaryLoaders,
    api_wind_runtime_loader_test::frame,
    installed_family_catalog_verification::verify_installed_active_canary,
    maintenance_canonical_json::{field, text},
};

struct Material {
    document: JsonValue,
    bytes: Vec<u8>,
    sha256: String,
    base64: String,
}

struct Oracle {
    root: JsonValue,
    materials: Vec<Material>,
    hashes: Vec<(Vec<u8>, String)>,
}

impl Oracle {
    /// Return one captured canonical material by its decoded value.
    fn material(&self, predicate: impl Fn(&JsonValue) -> bool) -> &Material {
        self.materials
            .iter()
            .find(|material| predicate(&material.document))
            .expect("captured Node material")
    }

    /// Return the selected generated package for one family.
    fn package(&self, family: &str) -> &Material {
        self.material(|document| {
            text(document, "family").as_deref() == Some(family)
                && field(document, "bundleSha256").is_some()
                && field(document, "source").is_some()
        })
    }

    /// Return one family's compiled public registry.
    fn registry(&self, family: &str) -> &Material {
        let expected = format!("forecast-adjustment-{family}-maintenance-registry/v1");
        self.material(|document| text(document, "contractVersion").as_deref() == Some(&expected))
    }

    /// Return one family's exact promotion action.
    fn action(&self, family: &str) -> &Material {
        self.material(|document| {
            text(document, "contractVersion").as_deref()
                == Some("forecast-adjustment-model-action/v1")
                && text(document, "family").as_deref() == Some(family)
        })
    }

    /// Return the catalog document carrying both independent active slots.
    fn catalog(&self) -> &Material {
        self.material(|document| {
            text(document, "contractVersion").as_deref()
                == Some("adjustment-installed-candidate-catalog/v2")
                && matches!(field(document, "entries"), Some(JsonValue::Array(entries)) if !entries.is_empty())
        })
    }

    /// Resolve one original canonical clock through Node's own Date implementation.
    fn clock(&self, value: &str) -> Option<i64> {
        let JsonValue::Number(epoch) = field(field(&self.root, "clocks")?, value)? else {
            return None;
        };
        Some(*epoch as i64)
    }

    /// Return exact canonical bytes for a captured decoded value.
    fn encode(&self, value: &JsonValue) -> Option<Vec<u8>> {
        self.materials
            .iter()
            .find(|material| &material.document == value)
            .map(|material| material.bytes.clone())
    }

    /// Return the original SHA-256 for an exact captured preimage.
    fn sha256(&self, bytes: &[u8]) -> Option<Sha256Digest> {
        self.hashes
            .iter()
            .find(|(preimage, _)| preimage == bytes)
            .map(|(_, digest)| Sha256Digest::admitted(digest.as_str()))
    }
}

struct Codec(&'static Oracle);

impl MaintenanceCanonicalJson for Codec {
    /// Decode only exact immutable bytes emitted by the original Node fixture.
    fn parse_canonical_object(&mut self, bytes: &[u8]) -> Option<JsonValue> {
        self.0
            .materials
            .iter()
            .find(|material| material.bytes == bytes)
            .map(|material| material.document.clone())
    }

    /// Re-emit only captured canonical values through their exact source bytes.
    fn encode_canonical_json(&mut self, value: &JsonValue) -> Option<Vec<u8>> {
        self.0.encode(value)
    }

    /// Reuse only digests computed by the original implementation.
    fn sha256(&mut self, bytes: &[u8]) -> Option<Sha256Digest> {
        self.0.sha256(bytes)
    }
}

impl ApiMaintenanceCandidateCodec for Codec {
    /// Admit only clocks that Node observed in the generated qualification graph.
    fn is_canonical_utc_instant(&mut self, value: &str) -> bool {
        self.0.clock(value).is_some()
    }

    /// Prove qualified serving does not call the retained private-package decoder.
    fn decode_maintenance_package(
        &mut self,
        _: &JsonValue,
    ) -> Option<ForecastAdjustmentMaintenanceRuntimePackage> {
        None
    }
}

#[derive(Clone, Copy)]
struct Values {
    oracle: &'static Oracle,
    calendar_matches: bool,
}

impl ApiMaintenanceReceiptCanonicalValues for Values {
    /// Resolve only original fixture clocks.
    fn canonical_utc_epoch_milliseconds(&self, value: &str) -> Option<i64> {
        self.oracle.clock(value)
    }

    /// Resolve only original fixture preimages.
    fn sha256(&self, bytes: &[u8]) -> Option<Sha256Digest> {
        self.oracle.sha256(bytes)
    }

    /// Qualified serving never creates a new database receipt.
    fn unsigned_receipt_sha256(
        &self,
        _: &[(&str, MaintenanceReceiptScalar<'_>)],
    ) -> Option<Sha256Digest> {
        None
    }
}

impl ApiMaintenanceShadowEvidenceValues for Values {
    /// Decode one exact captured JSON member.
    fn decode_shadow_json(&self, bytes: &[u8]) -> Option<JsonValue> {
        self.oracle
            .materials
            .iter()
            .find(|material| material.bytes == bytes)
            .map(|material| material.document.clone())
    }

    /// Ordered private evidence is outside this public package fixture.
    fn encode_shadow_ordered_json(&self, _: &JsonValue) -> Option<Vec<u8>> {
        None
    }

    /// Qualified serving does not decode embedded comparator material.
    fn decode_shadow_comparator_base64(&self, _: &[u16]) -> Option<Vec<u8>> {
        None
    }

    /// Preserve the exact canonical source bytes for every captured subtree.
    fn encode_shadow_sorted_json(&self, value: &JsonValue) -> Option<Vec<u8>> {
        self.oracle.encode(value)
    }

    /// Prove qualified serving does not call the retained shadow-package decoder.
    fn decode_shadow_maintenance_package(
        &self,
        _: &JsonValue,
    ) -> Option<ForecastAdjustmentMaintenanceRuntimePackage> {
        None
    }

    /// Qualified API canaries never decode a rain artifact.
    fn decode_shadow_rain_artifact(&self, _: &JsonValue) -> Option<RainHurdleWindPortableArtifact> {
        None
    }

    /// The package verifier owns wind string validation in this fixture.
    fn check_legacy_wind_runtime_value(&self, _: &[u16]) -> Option<()> {
        None
    }
}

impl ApiStandardRuntimeValues for Values {
    /// Preserve the process-calendar decision as an independently injectable effect.
    fn standard_calendar_matches(&self, _: &str, _: &str) -> bool {
        self.calendar_matches
    }

    /// Return Node's exact padded base64 for one admitted member.
    fn encode_standard_base64(&self, bytes: &[u8]) -> Option<String> {
        self.oracle
            .materials
            .iter()
            .find(|material| material.bytes == bytes)
            .map(|material| material.base64.clone())
    }
}

struct Files {
    oracle: &'static Oracle,
    catalog: Vec<u8>,
    catalog_reads: Rc<Cell<usize>>,
    member_reads: Rc<Cell<usize>>,
    damage_bundle: bool,
}

impl ApiMaintenanceCandidateFiles for Files {
    /// Return the selected immutable catalog bytes without filesystem substitution.
    fn read_catalog(&mut self) -> Result<Vec<u8>, ApiMaintenanceCandidateError> {
        self.catalog_reads.set(self.catalog_reads.get() + 1);
        Ok(self.catalog.clone())
    }

    /// The qualified loader must use its narrower three-member custody port.
    fn read_members(
        &mut self,
        _: AdjustmentShadowFamily,
        _: &str,
        _: &str,
        _: &str,
    ) -> Result<ApiMaintenanceCandidateMembers, ApiMaintenanceCandidateError> {
        unreachable!("qualified canary loader used candidate shadow custody")
    }
}

impl ApiQualifiedCanaryFiles for Files {
    /// Return exact captured registry, action, and package bytes for the selected identities.
    fn read_qualified_canary_members(
        &mut self,
        family: AdjustmentShadowFamily,
        action_sha256: &str,
        bundle_sha256: &str,
    ) -> Result<ApiQualifiedCanaryMembers, ApiMaintenanceCandidateError> {
        self.member_reads.set(self.member_reads.get() + 1);
        let family = family_name(family);
        let action = self.oracle.action(family);
        let package = self.oracle.package(family);
        assert_eq!(action.sha256, action_sha256);
        assert_eq!(
            text(&package.document, "bundleSha256").as_deref(),
            Some(bundle_sha256)
        );
        let mut bundle = package.bytes.clone();
        // make the selected package differ from its catalog-bound canonical member
        if self.damage_bundle {
            bundle.push(b'\n');
        }
        Ok(ApiQualifiedCanaryMembers {
            registry: self.oracle.registry(family).bytes.clone(),
            action: action.bytes.clone(),
            bundle,
        })
    }
}

/// Return the wire family spelling used by the captured Node graph.
fn family_name(family: AdjustmentShadowFamily) -> &'static str {
    match family {
        AdjustmentShadowFamily::Wind => "wind",
        AdjustmentShadowFamily::Temperature => "temperature",
        AdjustmentShadowFamily::Rain => "rain",
    }
}

/// Load the original package builder and canonical JSON counterpart once.
fn oracle() -> &'static Oracle {
    static ORACLE: OnceLock<Oracle> = OnceLock::new();
    ORACLE.get_or_init(|| {
        let script = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/qualified-canary-loader-node-oracle.mjs");
        let output = Command::new("node")
            .arg(script)
            .output()
            .expect("run qualified canary Node oracle");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let body = String::from_utf8(output.stdout).expect("Node fixture frame text");
        let mut lines = body.lines();
        let root = frame(&mut lines);
        assert!(lines.next().is_none());
        let Some(JsonValue::Array(values)) = field(&root, "materials") else {
            panic!("missing material frames")
        };
        let materials = values
            .iter()
            .map(|value| Material {
                document: field(value, "value").unwrap().clone(),
                bytes: byte_array(field(value, "bytes").unwrap()),
                sha256: text(value, "sha256").unwrap(),
                base64: text(value, "base64").unwrap(),
            })
            .collect();
        let Some(JsonValue::Array(values)) = field(&root, "hashes") else {
            panic!("missing hash frames")
        };
        let hashes = values
            .iter()
            .map(|value| {
                (
                    byte_array(field(value, "bytes").unwrap()),
                    text(value, "sha256").unwrap(),
                )
            })
            .collect();
        Oracle {
            root,
            materials,
            hashes,
        }
    })
}

/// Decode one Node-framed byte array without text round-tripping.
fn byte_array(value: &JsonValue) -> Vec<u8> {
    let JsonValue::Array(values) = value else {
        panic!("expected byte array")
    };
    values
        .iter()
        .map(|value| {
            let JsonValue::Number(value) = value else {
                panic!("expected byte value")
            };
            u8::try_from(*value as u64).expect("byte range")
        })
        .collect()
}

/// An injected loader with observable catalog and member read counts.
type ObservedLoader = (
    NativeInstalledQualifiedCanaryLoaders<Files, Codec, Values>,
    Rc<Cell<usize>>,
    Rc<Cell<usize>>,
);

/// Create one injected-custody file owner and retain its read counters.
fn files(catalog: Vec<u8>, damage_bundle: bool) -> (Files, Rc<Cell<usize>>, Rc<Cell<usize>>) {
    let catalog_reads = Rc::new(Cell::new(0));
    let member_reads = Rc::new(Cell::new(0));
    let fixture = oracle();
    (
        Files {
            oracle: fixture,
            catalog,
            catalog_reads: Rc::clone(&catalog_reads),
            member_reads: Rc::clone(&member_reads),
            damage_bundle,
        },
        catalog_reads,
        member_reads,
    )
}

/// Create one injected-custody loader and retain its read counters.
fn loader(catalog: Vec<u8>, calendar_matches: bool) -> ObservedLoader {
    let fixture = oracle();
    let (files, catalog_reads, member_reads) = files(catalog, false);
    (
        NativeInstalledQualifiedCanaryLoaders::new(
            files,
            Codec(fixture),
            Values {
                oracle: fixture,
                calendar_matches,
            },
        ),
        catalog_reads,
        member_reads,
    )
}

/// Read one family's installed receipt from the captured catalog.
fn receipt<'a>(oracle: &'a Oracle, family: &str) -> &'a JsonValue {
    let Some(JsonValue::Array(entries)) = field(&oracle.catalog().document, "entries") else {
        panic!("catalog entries")
    };
    entries
        .iter()
        .find(|entry| text(entry, "family").as_deref() == Some(family))
        .and_then(|entry| field(entry, "receipt"))
        .expect("family receipt")
}

#[test]
fn loads_actual_generated_wind_and_temperature_qualification_packages() {
    let oracle = oracle();
    let (mut loader, catalog_reads, member_reads) = loader(oracle.catalog().bytes.clone(), true);

    let wind = loader
        .load_qualified_wind_canary()
        .expect("qualified wind package");
    let LoadedForecastAdjustmentWindCanaryRuntime::MaintenanceActive(wind) = wind else {
        panic!("expected qualified wind runtime")
    };
    let wind_package = oracle.package("wind");
    let wind_receipt = receipt(oracle, "wind");
    assert_eq!(
        wind.bundle.bundleSha256,
        text(&wind_package.document, "bundleSha256").unwrap()
    );
    assert_eq!(wind.bundle.candidate.enabledMetricBands.len(), 13);
    assert_eq!(
        wind.bundle.maintenanceAuthority.actionSha256,
        text(wind_receipt, "actionSha256").unwrap()
    );
    assert_eq!(
        wind.comparatorAuthority.artifactMemberSha256,
        Nullable::Value(wind_package.sha256.clone())
    );
    assert_eq!(
        wind.comparatorAuthority.authorityKind,
        MaintenanceShadowComparatorAuthorityKind::MaintenanceQualified
    );

    let temperature = loader
        .load_qualified_temperature_canary()
        .expect("qualified temperature package");
    let LoadedForecastAdjustmentTemperatureCanaryRuntime::MaintenanceActive(temperature) =
        temperature
    else {
        panic!("expected qualified temperature runtime")
    };
    let temperature_package = oracle.package("temperature");
    assert_eq!(
        temperature.bundle.bundleSha256,
        text(&temperature_package.document, "bundleSha256").unwrap()
    );
    let Nullable::Value(coefficients) = temperature.bundle.model.directCoefficients else {
        panic!("generated temperature coefficients")
    };
    assert_eq!(coefficients.len(), 35);
    assert_eq!(
        temperature.comparatorAuthority.artifactMemberSha256,
        Nullable::Value(temperature_package.sha256.clone())
    );
    assert_eq!((catalog_reads.get(), member_reads.get()), (2, 2));
}

#[test]
fn omits_ordinary_missing_qualification_without_member_reads() {
    let oracle = oracle();
    let empty_catalog = byte_array(field(&oracle.root, "emptyCatalogBytes").unwrap());
    let (mut loader, catalog_reads, member_reads) = loader(empty_catalog, true);

    assert!(loader.load_qualified_wind_canary().is_err());
    assert!(loader.load_qualified_temperature_canary().is_err());
    assert_eq!((catalog_reads.get(), member_reads.get()), (2, 0));
}

#[test]
fn rejects_generated_wind_package_when_the_process_calendar_differs() {
    let oracle = oracle();
    let (mut loader, _, member_reads) = loader(oracle.catalog().bytes.clone(), false);

    assert!(loader.load_qualified_wind_canary().is_err());
    assert_eq!(member_reads.get(), 1);
}

/// Prove strict startup verification admits both generated active API canaries.
#[test]
fn strictly_verifies_generated_wind_and_temperature_active_canaries() {
    let fixture = oracle();
    let (mut files, catalog_reads, member_reads) = files(fixture.catalog().bytes.clone(), false);
    let mut codec = Codec(fixture);

    // verify each independent selected API family through the strict startup path
    for family in [
        AdjustmentShadowFamily::Temperature,
        AdjustmentShadowFamily::Wind,
    ] {
        verify_installed_active_canary(family, &mut files, &mut codec)
            .expect("strict active canary verification");
    }

    assert_eq!((catalog_reads.get(), member_reads.get()), (2, 2));
}

/// Prove strict startup verification rejects a damaged selected package member.
#[test]
fn strict_active_canary_verification_rejects_damaged_selected_bundle() {
    let fixture = oracle();
    let (mut files, catalog_reads, member_reads) = files(fixture.catalog().bytes.clone(), true);
    let mut codec = Codec(fixture);

    assert_eq!(
        verify_installed_active_canary(AdjustmentShadowFamily::Wind, &mut files, &mut codec),
        Err(ApiMaintenanceCandidateError::Canonical)
    );
    assert_eq!((catalog_reads.get(), member_reads.get()), (1, 1));
}

/// Prove a genuinely absent active selection succeeds without reading package members.
#[test]
fn strict_active_canary_verification_accepts_absent_selection() {
    let fixture = oracle();
    let empty_catalog = byte_array(field(&fixture.root, "emptyCatalogBytes").unwrap());
    let (mut files, catalog_reads, member_reads) = files(empty_catalog, false);
    let mut codec = Codec(fixture);

    assert_eq!(
        verify_installed_active_canary(AdjustmentShadowFamily::Temperature, &mut files, &mut codec,),
        Ok(())
    );
    assert_eq!((catalog_reads.get(), member_reads.get()), (1, 0));
}
