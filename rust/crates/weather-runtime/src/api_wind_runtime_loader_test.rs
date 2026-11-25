//! Differential file-selection tests for the installed legacy wind canary runtime.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::OnceLock,
    time::{SystemTime, UNIX_EPOCH},
};

use weather_api::WeatherApiStartupLoadError;
use weather_contracts::*;
use weather_database::Sha256Digest;

use crate::api_wind_runtime_projection::project_verified_wind_bundle;
use crate::maintenance_canonical_json::{field, text};
use crate::{
    ApiMaintenanceReceiptCanonicalValues, ApiMaintenanceShadowEvidenceValues,
    ApiStandardRuntimeValues, MaintenanceReceiptScalar, NativeForecastRuntimeFiles,
    load_installed_wind_runtime,
};

struct Oracle {
    hashes: Vec<(Vec<u8>, String)>,
    clocks: Vec<(String, i64)>,
    registry: JsonValue,
    registry_bytes: Vec<u8>,
    inactive_registry: JsonValue,
    inactive_registry_bytes: Vec<u8>,
    raw_registry: JsonValue,
    raw_registry_bytes: Vec<u8>,
    raw_registry_base64: String,
    raw_registry_member_sha256: String,
    finite_registry: JsonValue,
    finite_registry_bytes: Vec<u8>,
    permanent_bundle: JsonValue,
    permanent_bundle_bytes: Vec<u8>,
    finite_bundle: JsonValue,
    finite_bundle_bytes: Vec<u8>,
    bundle_base64: String,
    bundle_member_sha256: String,
    authorization: JsonValue,
    authorization_bytes: Vec<u8>,
    authorization_base64: String,
    authorization_member_sha256: String,
}

struct Fixture {
    oracle: Oracle,
}

impl ApiMaintenanceReceiptCanonicalValues for Oracle {
    /// Resolve only UTC spellings computed by the original Node calendar.
    fn canonical_utc_epoch_milliseconds(&self, value: &str) -> Option<i64> {
        self.clocks
            .iter()
            .find(|(clock, _)| clock == value)
            .map(|(_, epoch)| *epoch)
    }

    /// Accept only preimages whose SHA-256 was computed by the original implementation.
    fn sha256(&self, bytes: &[u8]) -> Option<Sha256Digest> {
        let (_, digest) = self.hashes.iter().find(|(preimage, _)| preimage == bytes)?;
        Some(Sha256Digest::admitted(digest.as_str()))
    }

    /// Installed selection does not create a database maintenance receipt.
    fn unsigned_receipt_sha256(
        &self,
        _: &[(&str, MaintenanceReceiptScalar<'_>)],
    ) -> Option<Sha256Digest> {
        unreachable!()
    }
}

impl ApiMaintenanceShadowEvidenceValues for Oracle {
    /// Decode only the exact canonical fixture members admitted by Node.
    fn decode_shadow_json(&self, bytes: &[u8]) -> Option<JsonValue> {
        if bytes == self.registry_bytes {
            return Some(self.registry.clone());
        }
        if bytes == self.inactive_registry_bytes {
            return Some(self.inactive_registry.clone());
        }
        if bytes == self.raw_registry_bytes {
            return Some(self.raw_registry.clone());
        }
        if bytes == self.finite_registry_bytes {
            return Some(self.finite_registry.clone());
        }
        if bytes == self.permanent_bundle_bytes {
            return Some(self.permanent_bundle.clone());
        }
        if bytes == self.finite_bundle_bytes {
            return Some(self.finite_bundle.clone());
        }
        // admit one malformed decoded object for the invalid-file disposition
        (bytes == b"{}\n").then(|| JsonValue::Object(Vec::new()))
    }

    /// Preserve the original object order when a caller requires a no-LF wire value.
    fn encode_shadow_ordered_json(&self, document: &JsonValue) -> Option<Vec<u8>> {
        self.known_document_bytes(document)
            .map(without_final_line_feed)
            .or_else(|| (document == &JsonValue::Object(Vec::new())).then(|| b"{}".to_vec()))
    }

    /// Installed selection never decodes an embedded comparator value.
    fn decode_shadow_comparator_base64(&self, _: &[u16]) -> Option<Vec<u8>> {
        unreachable!()
    }

    /// Preserve exact source bytes for whole documents and stable test bytes for subtrees.
    fn encode_shadow_sorted_json(&self, document: &JsonValue) -> Option<Vec<u8>> {
        if document == &self.authorization {
            return Some(self.authorization_bytes.clone());
        }
        self.known_document_bytes(document)
            .map(ToOwned::to_owned)
            .or_else(|| {
                (document == &JsonValue::Object(Vec::new()))
                    .then(|| b"{}\n".to_vec())
                    .or_else(|| Some(fixture_bytes(document)))
            })
    }

    /// A legacy canary bundle is not a qualified maintenance package.
    fn decode_shadow_maintenance_package(
        &self,
        _: &JsonValue,
    ) -> Option<ForecastAdjustmentMaintenanceRuntimePackage> {
        None
    }

    /// A wind canary does not carry a portable rain artifact.
    fn decode_shadow_rain_artifact(&self, _: &JsonValue) -> Option<RainHurdleWindPortableArtifact> {
        None
    }

    /// The original Node verifier already admitted every source fixture string.
    fn check_legacy_wind_runtime_value(&self, _: &[u16]) -> Option<()> {
        Some(())
    }
}

impl ApiStandardRuntimeValues for Oracle {
    /// Admit only the exact fitted calendar fingerprint carried by either source bundle.
    fn standard_calendar_matches(&self, icu_version: &str, tzdata_version: &str) -> bool {
        let candidate = field(&self.permanent_bundle, "candidate").unwrap();
        let fingerprint = field(candidate, "runtimeFingerprint").unwrap();
        text(fingerprint, "icuVersion").as_deref() == Some(icu_version)
            && text(fingerprint, "tzdataVersion").as_deref() == Some(tzdata_version)
    }

    /// Return Node's exact padded base64 for admitted legacy comparator members.
    fn encode_standard_base64(&self, bytes: &[u8]) -> Option<String> {
        if bytes == self.permanent_bundle_bytes {
            return Some(self.bundle_base64.clone());
        }
        if bytes == self.authorization_bytes {
            return Some(self.authorization_base64.clone());
        }
        (bytes == self.raw_registry_bytes).then(|| self.raw_registry_base64.clone())
    }
}

impl Oracle {
    /// Return exact canonical bytes for one complete installed document.
    fn known_document_bytes(&self, document: &JsonValue) -> Option<&[u8]> {
        if document == &self.registry {
            return Some(&self.registry_bytes);
        }
        if document == &self.inactive_registry {
            return Some(&self.inactive_registry_bytes);
        }
        if document == &self.raw_registry {
            return Some(&self.raw_registry_bytes);
        }
        if document == &self.finite_registry {
            return Some(&self.finite_registry_bytes);
        }
        if document == &self.permanent_bundle {
            return Some(&self.permanent_bundle_bytes);
        }
        (document == &self.finite_bundle).then_some(self.finite_bundle_bytes.as_slice())
    }
}

/// Remove the one canonical JSON terminator without altering member bytes.
fn without_final_line_feed(bytes: &[u8]) -> Vec<u8> {
    bytes.strip_suffix(b"\n").unwrap_or(bytes).to_vec()
}

/// Obtain a deterministic key for Node-provided canonical subtrees.
fn fixture_bytes(value: &JsonValue) -> Vec<u8> {
    format!("{value:?}").into_bytes()
}

/// Read one value in the bounded test-only line protocol.
pub(crate) fn frame<'a>(lines: &mut impl Iterator<Item = &'a str>) -> JsonValue {
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

/// Preserve every original UTF-16 code unit without lossy UTF-8 conversion.
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

/// Load checked-in source bytes and original canonical values once for this test module.
fn fixture() -> &'static Fixture {
    static FIXTURE: OnceLock<Fixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let script =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/wind-runtime-loader-node-oracle.mjs");
        let output = Command::new("node")
            .arg(script)
            .output()
            .expect("run wind runtime Node oracle");
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
            permanent_bundle,
            finite_bundle,
            JsonValue::Array(hashes),
            JsonValue::Object(clocks),
            JsonValue::Object(installed),
        ] = <[_; 5]>::try_from(values).expect("five Node fixture slots")
        else {
            panic!("Node fixture shape")
        };
        let mut hashes: Vec<(Vec<u8>, String)> = hashes
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
            .map(|(clock, epoch)| {
                let JsonValue::Number(epoch) = epoch else {
                    panic!("numeric clock frame")
                };
                (String::from_utf16(&clock).unwrap(), epoch as i64)
            })
            .collect();
        let installed = JsonValue::Object(installed);
        let registry = field(&installed, "registry").unwrap().clone();
        let registry_bytes = byte_array(field(&installed, "registryBytes").unwrap());
        let inactive_registry = field(&installed, "inactiveRegistry").unwrap().clone();
        let inactive_registry_bytes =
            byte_array(field(&installed, "inactiveRegistryBytes").unwrap());
        let raw_registry = field(&installed, "rawRegistry").unwrap().clone();
        let raw_registry_bytes = byte_array(field(&installed, "rawRegistryBytes").unwrap());
        let raw_registry_member_sha256 = text(&installed, "rawRegistryMemberSha256").unwrap();
        let finite_registry = field(&installed, "finiteRegistry").unwrap().clone();
        let finite_registry_bytes = byte_array(field(&installed, "finiteRegistryBytes").unwrap());
        let permanent_bundle_bytes = byte_array(field(&installed, "bundleBytes").unwrap());
        let finite_bundle_bytes = byte_array(field(&installed, "finiteBundleBytes").unwrap());
        let authorization = field(&permanent_bundle, "authorization").unwrap().clone();
        let authorization_bytes = byte_array(field(&installed, "authorizationBytes").unwrap());
        let bundle_member_sha256 = text(&installed, "bundleMemberSha256").unwrap();
        let authorization_member_sha256 = text(&installed, "authorizationMemberSha256").unwrap();
        // bind exact file bytes in addition to canonical decoded hash preimages
        hashes.push((permanent_bundle_bytes.clone(), bundle_member_sha256.clone()));
        hashes.push((
            authorization_bytes.clone(),
            authorization_member_sha256.clone(),
        ));
        hashes.push((
            raw_registry_bytes.clone(),
            raw_registry_member_sha256.clone(),
        ));
        Fixture {
            oracle: Oracle {
                hashes,
                clocks,
                registry,
                registry_bytes,
                inactive_registry,
                inactive_registry_bytes,
                raw_registry,
                raw_registry_bytes,
                raw_registry_base64: text(&installed, "rawRegistryBase64").unwrap(),
                raw_registry_member_sha256,
                finite_registry,
                finite_registry_bytes,
                permanent_bundle,
                permanent_bundle_bytes,
                finite_bundle,
                finite_bundle_bytes,
                bundle_base64: text(&installed, "bundleBase64").unwrap(),
                bundle_member_sha256,
                authorization,
                authorization_bytes,
                authorization_base64: text(&installed, "authorizationBase64").unwrap(),
                authorization_member_sha256,
            },
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

/// Own one disposable canonical installed-runtime tree per behavior test.
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
            "weather-wind-runtime-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(root.join("ballydidean/wind-canary-bundles"))
            .expect("create installed wind runtime fixture");
        Self { root }
    }

    /// Return the fixed installed wind registry member.
    fn registry_path(&self) -> PathBuf {
        self.root.join("ballydidean-wind-canary.json")
    }

    /// Return one content-addressed wind canary bundle member.
    fn bundle_path(&self, digest: &str) -> PathBuf {
        self.root
            .join("ballydidean/wind-canary-bundles")
            .join(format!("sha256-{digest}.json"))
    }
}

impl Drop for InstalledRuntimeDirectory {
    /// Remove the bounded disposable fixture tree after every test outcome.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// Return one qualified fallback closure that records unexpected delegation.
fn never_qualified(
    calls: &mut usize,
) -> impl FnMut() -> Result<LoadedForecastAdjustmentWindCanaryRuntime, WeatherApiStartupLoadError> + '_
{
    move || {
        *calls += 1;
        panic!("legacy registry must not delegate to qualified wind selection")
    }
}

/// Require one exact legacy disabled disposition with no comparator authority.
fn assert_disabled(
    loaded: LoadedForecastAdjustmentWindCanaryRuntime,
    expected: LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledReasonCodeEnum,
) {
    let LoadedForecastAdjustmentWindCanaryRuntime::Legacy {
        value: LoadedForecastAdjustmentWindCanaryRuntimeV1::Disabled(disabled),
        comparatorAuthority: FieldPresence::Missing,
    } = loaded
    else {
        panic!("expected disabled legacy wind runtime")
    };
    assert_eq!(disabled.reasonCode, expected);
}

/// Load the checked-in permanent bundle with its exact Node-derived comparator members.
#[test]
fn loads_checked_in_permanent_wind_runtime_with_node_authority() {
    let fixture = fixture();
    let directory = InstalledRuntimeDirectory::create();
    let bundle_sha256 = text(&fixture.oracle.permanent_bundle, "bundleSha256").unwrap();
    fs::write(directory.registry_path(), &fixture.oracle.registry_bytes)
        .expect("write checked-in wind registry");
    fs::write(
        directory.bundle_path(&bundle_sha256),
        &fixture.oracle.permanent_bundle_bytes,
    )
    .expect("write checked-in permanent bundle");
    let files = NativeForecastRuntimeFiles::new(directory.root.clone())
        .expect("bind installed wind runtime fixture");
    let activated_at = text(
        field(&fixture.oracle.permanent_bundle, "authorization").unwrap(),
        "activatedAt",
    )
    .unwrap();
    let mut now_calls = 0;
    let mut qualified_calls = 0;
    let loaded = {
        let mut now = || {
            now_calls += 1;
            Some(activated_at.clone())
        };
        let mut qualified = never_qualified(&mut qualified_calls);
        load_installed_wind_runtime(&files, &fixture.oracle, None, &mut now, &mut qualified)
    };
    let LoadedForecastAdjustmentWindCanaryRuntime::Legacy {
        value: LoadedForecastAdjustmentWindCanaryRuntimeV1::Active(active),
        comparatorAuthority: FieldPresence::Value(authority),
    } = loaded
    else {
        panic!("expected active checked-in wind runtime")
    };
    let ForecastAdjustmentWindCanaryRuntimeBundle::Permanent(bundle) = active.bundle else {
        panic!("checked-in wind runtime must remain permanent")
    };
    assert_eq!(bundle.bundleSha256, bundle_sha256);
    assert_eq!(
        bundle.candidate.candidateArtifactSha256,
        "454e7b91a34407a6ace79150dfc5799c37ced2adbdab915b710f18180c3cd488"
    );
    assert_eq!(now_calls, 1);
    assert_eq!(qualified_calls, 0);
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
        Nullable::Value(bundle_sha256)
    );
    assert_eq!(
        authority.artifactMemberSha256,
        Nullable::Value(fixture.oracle.bundle_member_sha256.clone())
    );
    assert_eq!(authority.receiptBase64, fixture.oracle.authorization_base64);
    assert_eq!(
        authority.receiptMemberSha256,
        fixture.oracle.authorization_member_sha256
    );
}

/// Project the checked-in finite generation without any fixture-owned typed decoder.
#[test]
fn projects_checked_in_finite_wind_runtime_natively() {
    let fixture = fixture();
    let projected = project_verified_wind_bundle(&fixture.oracle.finite_bundle)
        .expect("project checked-in finite wind runtime");
    let ForecastAdjustmentWindCanaryRuntimeBundle::TimeLimited(bundle) = projected else {
        panic!("checked-in finite wind runtime must remain time limited")
    };
    assert_eq!(
        bundle.bundleSha256,
        text(&fixture.oracle.finite_bundle, "bundleSha256").unwrap()
    );
    assert!(!bundle.candidate.coefficients.is_empty());
    assert!(!bundle.transferReport.bridgeEvaluations.is_empty());
    assert_eq!(
        bundle.authorization.expiresAt,
        text(
            field(&fixture.oracle.finite_bundle, "authorization").unwrap(),
            "expiresAt",
        )
        .unwrap()
    );
}

/// Reject the checked-in finite bundle at its exact exclusive deadline.
#[test]
fn expires_checked_in_finite_wind_runtime_at_exact_deadline() {
    let fixture = fixture();
    let directory = InstalledRuntimeDirectory::create();
    let bundle_sha256 = text(&fixture.oracle.finite_bundle, "bundleSha256").unwrap();
    fs::write(
        directory.registry_path(),
        &fixture.oracle.finite_registry_bytes,
    )
    .expect("write finite wind registry");
    fs::write(
        directory.bundle_path(&bundle_sha256),
        &fixture.oracle.finite_bundle_bytes,
    )
    .expect("write checked-in finite bundle");
    let files = NativeForecastRuntimeFiles::new(directory.root.clone())
        .expect("bind finite wind runtime fixture");
    let deadline = text(
        field(&fixture.oracle.finite_bundle, "authorization").unwrap(),
        "expiresAt",
    )
    .unwrap();
    let mut now = || Some(deadline.clone());
    let mut qualified_calls = 0;
    let loaded = {
        let mut qualified = never_qualified(&mut qualified_calls);
        load_installed_wind_runtime(&files, &fixture.oracle, None, &mut now, &mut qualified)
    };
    assert_disabled(
        loaded,
        LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledReasonCodeEnum::CanaryExpired,
    );
    assert_eq!(qualified_calls, 0);
}

/// Preserve the original immutable v2 raw registry receipt and comparator authority.
#[test]
fn loads_original_raw_v2_registry_without_clock_or_qualification() {
    let fixture = fixture();
    let directory = InstalledRuntimeDirectory::create();
    fs::write(
        directory.registry_path(),
        &fixture.oracle.raw_registry_bytes,
    )
    .expect("write original raw v2 registry");
    let files = NativeForecastRuntimeFiles::new(directory.root.clone())
        .expect("bind raw wind runtime fixture");
    let mut now_calls = 0;
    let mut qualified_calls = 0;
    let loaded = {
        let mut now = || {
            now_calls += 1;
            Some("2026-10-01T16:33:20.936Z".to_owned())
        };
        let mut qualified = never_qualified(&mut qualified_calls);
        load_installed_wind_runtime(&files, &fixture.oracle, None, &mut now, &mut qualified)
    };
    let LoadedForecastAdjustmentWindCanaryRuntime::MaintenanceDisabled(disabled) = loaded else {
        panic!("expected policy-raw wind runtime")
    };
    assert_eq!(
        disabled.comparatorAuthority.authorityKind,
        MaintenanceShadowComparatorAuthorityKind::PolicyRaw
    );
    assert_eq!(disabled.comparatorAuthority.artifactBase64, Nullable::Null);
    assert_eq!(
        disabled.comparatorAuthority.receiptBase64,
        fixture.oracle.raw_registry_base64
    );
    assert_eq!(
        disabled.comparatorAuthority.receiptMemberSha256,
        fixture.oracle.raw_registry_member_sha256
    );
    assert_eq!(now_calls, 0);
    assert_eq!(qualified_calls, 0);
}

/// Preserve pre-I/O kill, missing bundle, and reviewed inactive fallbacks.
#[test]
fn preserves_kill_missing_and_inactive_file_dispositions() {
    let fixture = fixture();
    let directory = InstalledRuntimeDirectory::create();
    let files = NativeForecastRuntimeFiles::new(directory.root.clone())
        .expect("bind empty installed wind runtime fixture");
    let mut now_calls = 0;
    let mut qualified_calls = 0;
    {
        let mut now = || {
            now_calls += 1;
            Some("2026-10-01T16:33:20.936Z".to_owned())
        };
        let mut qualified = never_qualified(&mut qualified_calls);
        assert_disabled(
            load_installed_wind_runtime(
                &files,
                &fixture.oracle,
                Some("1"),
                &mut now,
                &mut qualified,
            ),
            LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledReasonCodeEnum::CanaryKilled,
        );

        fs::write(directory.registry_path(), &fixture.oracle.registry_bytes)
            .expect("write active registry without bundle");
        assert_disabled(
            load_installed_wind_runtime(&files, &fixture.oracle, None, &mut now, &mut qualified),
            LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledReasonCodeEnum::BundleMissing,
        );

        fs::write(
            directory.registry_path(),
            &fixture.oracle.inactive_registry_bytes,
        )
        .expect("write inactive wind registry");
        assert_disabled(
            load_installed_wind_runtime(&files, &fixture.oracle, None, &mut now, &mut qualified),
            LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledReasonCodeEnum::RegistryInactive,
        );
    }
    assert_eq!(now_calls, 0);
    assert_eq!(qualified_calls, 0);
}
