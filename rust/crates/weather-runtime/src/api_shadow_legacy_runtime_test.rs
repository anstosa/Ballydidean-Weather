//! Differential fixture for the retained general wind runtime qualification graph.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use weather_contracts::*;
use weather_database::Sha256Digest;

use crate::api_shadow_legacy_runtime::verify_legacy_wind_runtime_bundle;
use crate::maintenance_canonical_json::{field, text};
use crate::{
    ApiMaintenanceReceiptCanonicalValues, ApiMaintenanceShadowEvidenceValues,
    ApiStandardRuntimeValues, MaintenanceReceiptScalar, NativeForecastRuntimeFiles,
    load_installed_standard_runtime,
};

struct Oracle {
    hashes: Vec<(Vec<u8>, String)>,
    clocks: Vec<(String, i64)>,
    registry: JsonValue,
    registry_bytes: Vec<u8>,
    inactive_registry: JsonValue,
    inactive_registry_bytes: Vec<u8>,
    bundle: JsonValue,
    bundle_bytes: Vec<u8>,
    bundle_base64: String,
    bundle_member_sha256: String,
    receipt: JsonValue,
    receipt_bytes: Vec<u8>,
    receipt_base64: String,
    receipt_member_sha256: String,
}

struct Fixture {
    valid: JsonValue,
    invalid: JsonValue,
    oracle: Oracle,
}

impl ApiMaintenanceReceiptCanonicalValues for Oracle {
    /// Resolve the exact clock spellings provided by Node.
    fn canonical_utc_epoch_milliseconds(&self, value: &str) -> Option<i64> {
        self.clocks
            .iter()
            .find(|(clock, _)| clock == value)
            .map(|(_, epoch)| *epoch)
    }

    /// Accept only the exact preimages whose real SHA-256 Node computed.
    fn sha256(&self, bytes: &[u8]) -> Option<Sha256Digest> {
        let (_, digest) = self.hashes.iter().find(|(preimage, _)| preimage == bytes)?;
        Some(Sha256Digest::admitted(digest.as_str()))
    }

    /// Qualification validation does not create new database receipts.
    fn unsigned_receipt_sha256(
        &self,
        _: &[(&str, MaintenanceReceiptScalar<'_>)],
    ) -> Option<Sha256Digest> {
        unreachable!()
    }
}

impl ApiMaintenanceShadowEvidenceValues for Oracle {
    /// Receive the original model through the independent test-only value stream.
    fn decode_shadow_json(&self, bytes: &[u8]) -> Option<JsonValue> {
        if bytes == self.registry_bytes {
            return Some(self.registry.clone());
        }
        if bytes == self.inactive_registry_bytes {
            return Some(self.inactive_registry.clone());
        }
        if bytes == b"{}\n" {
            return Some(JsonValue::Object(Vec::new()));
        }
        (bytes == self.bundle_bytes).then(|| self.bundle.clone())
    }
    /// No wire serializer is used by the general qualification graph.
    fn encode_shadow_ordered_json(&self, doc: &JsonValue) -> Option<Vec<u8>> {
        if doc == &self.registry {
            return Some(self.registry_bytes[..self.registry_bytes.len() - 1].to_vec());
        }
        if doc == &self.inactive_registry {
            return Some(
                self.inactive_registry_bytes[..self.inactive_registry_bytes.len() - 1].to_vec(),
            );
        }
        if doc == &JsonValue::Object(Vec::new()) {
            return Some(b"{}".to_vec());
        }
        (doc == &self.bundle).then(|| self.bundle_bytes[..self.bundle_bytes.len() - 1].to_vec())
    }
    /// The test bypasses the already verified comparator envelope.
    fn decode_shadow_comparator_base64(&self, _: &[u16]) -> Option<Vec<u8>> {
        unreachable!()
    }
    /// Key approved preimages by the Node-provided sorted tree, not by a test JSON implementation.
    fn encode_shadow_sorted_json(&self, doc: &JsonValue) -> Option<Vec<u8>> {
        if doc == &self.bundle {
            return Some(self.bundle_bytes.clone());
        }
        if doc == &self.registry {
            return Some(self.registry_bytes.clone());
        }
        if doc == &self.inactive_registry {
            return Some(self.inactive_registry_bytes.clone());
        }
        if doc == &JsonValue::Object(Vec::new()) {
            return Some(b"{}\n".to_vec());
        }
        if doc == &self.receipt {
            return Some(self.receipt_bytes.clone());
        }
        Some(fixture_bytes(doc))
    }
    /// The legacy runtime is not a qualified maintenance package.
    fn decode_shadow_maintenance_package(
        &self,
        _: &JsonValue,
    ) -> Option<ForecastAdjustmentMaintenanceRuntimePackage> {
        unreachable!()
    }
    /// The fixture carries fitted wind material rather than a rain artifact.
    fn decode_shadow_rain_artifact(&self, _: &JsonValue) -> Option<RainHurdleWindPortableArtifact> {
        unreachable!()
    }
    /// The source fixture has already passed the original Node filter.
    fn check_legacy_wind_runtime_value(&self, _: &[u16]) -> Option<()> {
        Some(())
    }
}

impl ApiStandardRuntimeValues for Oracle {
    /// Admit only the exact fingerprint carried by the verified fixture candidate.
    fn standard_calendar_matches(&self, icu_version: &str, tzdata_version: &str) -> bool {
        let candidate = field(&self.bundle, "candidate").unwrap();
        let fingerprint = field(candidate, "runtimeFingerprint").unwrap();
        text(fingerprint, "icuVersion").as_deref() == Some(icu_version)
            && text(fingerprint, "tzdataVersion").as_deref() == Some(tzdata_version)
    }

    /// Return Node's exact padded base64 only for admitted member bytes.
    fn encode_standard_base64(&self, bytes: &[u8]) -> Option<String> {
        if bytes == self.bundle_bytes {
            return Some(self.bundle_base64.clone());
        }
        (bytes == self.receipt_bytes).then(|| self.receipt_base64.clone())
    }
}

/// Obtain a deterministic key for exact sorted documents in the oracle fixture.
fn fixture_bytes(value: &JsonValue) -> Vec<u8> {
    format!("{value:?}").into_bytes()
}

/// Read one value in the test-only line protocol.
fn frame<'a>(lines: &mut impl Iterator<Item = &'a str>) -> JsonValue {
    let (kind, body) = lines.next().expect("missing Node frame").split_at(1);
    match kind {
        "z" => JsonValue::Null,
        "t" => JsonValue::Boolean(true),
        "f" => JsonValue::Boolean(false),
        "n" => JsonValue::Number(body.parse().expect("numeric frame")),
        "s" => JsonValue::String(utf16_frame(body)),
        "a" => JsonValue::Array(
            (0..body.parse::<usize>().unwrap())
                .map(|_| frame(lines))
                .collect(),
        ),
        "o" => JsonValue::Object(
            (0..body.parse::<usize>().unwrap())
                .map(|_| {
                    let JsonValue::String(key) = frame(lines) else {
                        panic!("object key frame")
                    };
                    (key, frame(lines))
                })
                .collect(),
        ),
        _ => panic!("unknown Node frame"),
    }
}

/// Preserve each test fixture's exact code unit without lossy UTF-8 conversion.
fn utf16_frame(body: &str) -> Vec<u16> {
    let (units, remainder) = body.as_bytes().as_chunks::<4>();
    assert!(remainder.is_empty());
    units
        .iter()
        .map(|unit| {
            u16::from_str_radix(std::str::from_utf8(unit).unwrap(), 16)
                .unwrap()
                .swap_bytes()
        })
        .collect()
}

/// Read a Node-approved artifact, a nonqualifying artifact, and their immutable preimages.
fn fixture() -> Fixture {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/legacy-runtime-node-oracle.mjs");
    let output = Command::new("node")
        .arg(script)
        .output()
        .expect("run legacy Node oracle");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let body = String::from_utf8(output.stdout).expect("Node frame text");
    let mut lines = body.lines();
    let JsonValue::Array(values) = frame(&mut lines) else {
        panic!("expected Node fixture array")
    };
    assert!(lines.next().is_none());
    let [
        valid,
        invalid,
        JsonValue::Array(hashes),
        JsonValue::Object(clocks),
        JsonValue::Object(installed),
    ] = <[_; 5]>::try_from(values).expect("five Node fixture slots")
    else {
        panic!("Node fixture shape")
    };
    let hashes: Vec<(Vec<u8>, String)> = hashes
        .into_iter()
        .map(|record| {
            let JsonValue::Array(fields) = record else {
                panic!("digest frame pair")
            };
            let [value, JsonValue::String(digest)] = <[_; 2]>::try_from(fields).unwrap() else {
                panic!("digest frame fields")
            };
            (fixture_bytes(&value), String::from_utf16(&digest).unwrap())
        })
        .collect();
    let clocks = clocks
        .into_iter()
        .map(|(name, value)| {
            let JsonValue::Number(epoch) = value else {
                panic!("numeric clock frame")
            };
            (String::from_utf16(&name).unwrap(), epoch as i64)
        })
        .collect();
    let installed = JsonValue::Object(installed);
    let registry = field(&installed, "registry").unwrap().clone();
    let registry_bytes = byte_array(field(&installed, "registryBytes").unwrap());
    let inactive_registry = field(&installed, "inactiveRegistry").unwrap().clone();
    let inactive_registry_bytes = byte_array(field(&installed, "inactiveRegistryBytes").unwrap());
    let bundle_bytes = byte_array(field(&installed, "bundleBytes").unwrap());
    let receipt_bytes = byte_array(field(&installed, "receiptBytes").unwrap());
    let bundle_base64 = text(&installed, "bundleBase64").unwrap();
    let receipt_base64 = text(&installed, "receiptBase64").unwrap();
    let bundle_member_sha256 = text(&installed, "bundleMemberSha256").unwrap();
    let receipt_member_sha256 = text(&installed, "receiptMemberSha256").unwrap();
    let receipt = field(&valid, "qualificationReceipt").unwrap().clone();
    let mut hashes = hashes;
    hashes.push((bundle_bytes.clone(), bundle_member_sha256.clone()));
    hashes.push((receipt_bytes.clone(), receipt_member_sha256.clone()));
    Fixture {
        invalid,
        oracle: Oracle {
            hashes,
            clocks,
            registry,
            registry_bytes,
            inactive_registry,
            inactive_registry_bytes,
            bundle: valid.clone(),
            bundle_bytes,
            bundle_base64,
            bundle_member_sha256,
            receipt,
            receipt_bytes,
            receipt_base64,
            receipt_member_sha256,
        },
        valid,
    }
}

/// Decode one Node-framed byte array without lossy text conversion.
fn byte_array(value: &JsonValue) -> Vec<u8> {
    let JsonValue::Array(values) = value else {
        panic!("expected byte array")
    };
    // preserve each canonical byte without passing through text decoding
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

/// Own one disposable canonical absolute installed-runtime tree.
struct InstalledRuntimeDirectory {
    root: PathBuf,
}

impl InstalledRuntimeDirectory {
    /// Create the exact registry and bundle directory layout used in production.
    fn create() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time after Unix epoch")
            .as_nanos();
        let root = PathBuf::from("/var/tmp").join(format!(
            "weather-standard-runtime-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(root.join("ballydidean/bundles"))
            .expect("create installed runtime fixture");
        Self { root }
    }

    /// Return the fixed installed registry member.
    fn registry_path(&self) -> PathBuf {
        self.root.join("ballydidean.json")
    }

    /// Return the only content-addressed bundle member selected by the registry.
    fn bundle_path(&self, digest: &str) -> PathBuf {
        self.root
            .join("ballydidean/bundles")
            .join(format!("sha256-{digest}.json"))
    }
}

impl Drop for InstalledRuntimeDirectory {
    /// Remove the bounded disposable tree after each test outcome.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// Require one exact disabled disposition from the installed loader.
fn assert_disabled(
    loaded: LoadedForecastAdjustmentRuntimeV1,
    expected: LoadedForecastAdjustmentRuntimeV1DisabledReasonCodeEnum,
) {
    let LoadedForecastAdjustmentRuntimeV1::Disabled(disabled) = loaded else {
        panic!("expected disabled standard runtime")
    };
    assert_eq!(disabled.reasonCode, expected);
}

/// Match the original passing graph and refuse below-floor fitted material even with matching hashes.
#[test]
fn matches_the_original_general_wind_runtime_qualification_graph() {
    let fixture = fixture();
    assert!(
        verify_legacy_wind_runtime_bundle(
            &fixture.oracle,
            &fixture.valid,
            &text(&fixture.valid, "bundleSha256").unwrap(),
        )
        .is_some()
    );
    assert!(
        verify_legacy_wind_runtime_bundle(
            &fixture.oracle,
            &fixture.invalid,
            &text(&fixture.invalid, "bundleSha256").unwrap(),
        )
        .is_none()
    );
}

/// load canonical node bytes and preserve each reviewed disabled fallback
#[test]
fn loads_node_verified_standard_runtime_from_installed_files() {
    let fixture = fixture();
    let directory = InstalledRuntimeDirectory::create();
    let bundle_sha256 = text(&fixture.valid, "bundleSha256").unwrap();
    let bundle_path = directory.bundle_path(&bundle_sha256);
    fs::write(directory.registry_path(), &fixture.oracle.registry_bytes)
        .expect("write active registry");
    fs::write(&bundle_path, &fixture.oracle.bundle_bytes).expect("write selected bundle");
    let files = NativeForecastRuntimeFiles::new(directory.root.clone())
        .expect("bind installed runtime fixture");

    let LoadedForecastAdjustmentRuntimeV1::Active(active) =
        load_installed_standard_runtime(&files, &fixture.oracle)
    else {
        panic!("expected active standard runtime")
    };
    assert_eq!(active.bundle.bundleSha256, bundle_sha256);
    assert_eq!(active.bundle.candidate.coefficients[0].coefficient, 2.0);
    let [evaluation] = active
        .bundle
        .evaluationReport
        .metricBandEvaluations
        .as_slice()
    else {
        panic!("expected one projected metric-band evaluation")
    };
    assert_eq!(
        evaluation.network,
        ForecastAdjustmentPairedScore {
            adjustedLoss: 9.0,
            bootstrapLowerBound: 0.01,
            bootstrapUpperBound: 0.2,
            eventCount: 100,
            rawLoss: 10.0,
            skill: 0.1,
        }
    );
    assert_eq!(evaluation.providerBalanced, evaluation.network);
    assert_eq!(
        evaluation.metricBand,
        ForecastAdjustmentMetricBand {
            leadBand: ForecastLeadBandKey::Value001024,
            metric: ForecastAdjustmentMetric::TemperatureC,
        }
    );
    assert_eq!(
        evaluation.evaluatedSeasonDaypartKeys,
        vec![ForecastSeasonDaypartKey::WinterNight]
    );
    assert_eq!(
        evaluation.scoreableStationKeys,
        vec![
            ForecastObservationStationKey::AmbientMaxweather,
            ForecastObservationStationKey::BallydideanEcowitt,
            ForecastObservationStationKey::NetatmoNearby,
            ForecastObservationStationKey::Tempest126537,
            ForecastObservationStationKey::Tempest168853,
        ]
    );
    assert_eq!(evaluation.criticalSlices.len(), 11);
    assert!(matches!(
        evaluation.criticalSlices.first(),
        Some(ForecastAdjustmentCriticalSliceScore::NearestThree(score))
            if score.eventCount == 100 && score.skill == 0.1
    ));
    // retain the exact provider slice order
    assert_eq!(
        evaluation.criticalSlices[1..5]
            .iter()
            .map(|slice| match slice {
                ForecastAdjustmentCriticalSliceScore::ProviderFamily(score) => score.key,
                _ => panic!("expected provider-family slice"),
            })
            .collect::<Vec<_>>(),
        vec![
            ForecastObservationProviderFamily::Ambient,
            ForecastObservationProviderFamily::Ecowitt,
            ForecastObservationProviderFamily::Netatmo,
            ForecastObservationProviderFamily::Tempest,
        ]
    );
    assert!(matches!(
        &evaluation.criticalSlices[5],
        ForecastAdjustmentCriticalSliceScore::SeasonDaypart(score)
            if score.key == ForecastSeasonDaypartKey::WinterNight
    ));
    // retain the exact station slice order
    assert_eq!(
        evaluation.criticalSlices[6..]
            .iter()
            .map(|slice| match slice {
                ForecastAdjustmentCriticalSliceScore::Station(score) => score.key,
                _ => panic!("expected station slice"),
            })
            .collect::<Vec<_>>(),
        evaluation.scoreableStationKeys
    );
    assert_eq!(active.bundle.qualificationReceipt.gates.len(), 7);
    // retain the generated gate order
    assert_eq!(
        active
            .bundle
            .qualificationReceipt
            .gates
            .iter()
            .map(|gate| gate.name)
            .collect::<Vec<_>>(),
        vec![
            ForecastAdjustmentQualificationGateName::PooledNetworkImprovement,
            ForecastAdjustmentQualificationGateName::BootstrapLowerBound,
            ForecastAdjustmentQualificationGateName::DevelopmentFoldSkill,
            ForecastAdjustmentQualificationGateName::CriticalSliceNoHarm,
            ForecastAdjustmentQualificationGateName::CoefficientCoverageAndCaps,
            ForecastAdjustmentQualificationGateName::LockedHoldout,
            ForecastAdjustmentQualificationGateName::ProductionIdentity,
        ]
    );
    assert!(
        active
            .bundle
            .qualificationReceipt
            .gates
            .iter()
            .all(|gate| gate.passed && matches!(&gate.reasonCode, Nullable::Null))
    );
    assert_eq!(
        active.bundle.qualificationReceipt.evidenceRedundancy.status,
        ForecastAdjustmentEvidenceRedundancyStatusEnum::IndependentContentAddressedCopy
    );
    let FieldPresence::Value(authority) = active.comparatorAuthority else {
        panic!("expected retained comparator authority")
    };
    assert_eq!(
        authority.authorityKind,
        MaintenanceShadowComparatorAuthorityKind::LegacyActive
    );
    assert_eq!(
        authority.artifactBase64,
        Nullable::Value(fixture.oracle.bundle_base64.clone())
    );
    assert_eq!(
        authority.artifactIdentitySha256,
        Nullable::Value(bundle_sha256.clone())
    );
    assert_eq!(
        authority.artifactMemberSha256,
        Nullable::Value(fixture.oracle.bundle_member_sha256.clone())
    );
    assert_eq!(authority.receiptBase64, fixture.oracle.receipt_base64);
    assert_eq!(
        authority.receiptMemberSha256,
        fixture.oracle.receipt_member_sha256
    );

    fs::write(
        directory.registry_path(),
        &fixture.oracle.inactive_registry_bytes,
    )
    .expect("write inactive registry");
    assert_disabled(
        load_installed_standard_runtime(&files, &fixture.oracle),
        LoadedForecastAdjustmentRuntimeV1DisabledReasonCodeEnum::RegistryInactive,
    );

    fs::write(directory.registry_path(), &fixture.oracle.registry_bytes)
        .expect("restore active registry");
    fs::remove_file(&bundle_path).expect("remove selected bundle");
    assert_disabled(
        load_installed_standard_runtime(&files, &fixture.oracle),
        LoadedForecastAdjustmentRuntimeV1DisabledReasonCodeEnum::BundleMissing,
    );

    fs::write(&bundle_path, b"{}\n").expect("write invalid selected bundle");
    assert_disabled(
        load_installed_standard_runtime(&files, &fixture.oracle),
        LoadedForecastAdjustmentRuntimeV1DisabledReasonCodeEnum::BundleInvalid,
    );
}
