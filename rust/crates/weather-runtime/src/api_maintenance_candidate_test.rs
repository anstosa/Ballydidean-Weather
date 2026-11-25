//! Exercise the real startup graph with injected original-format JSON and file owners.

use super::*;
use crate::ApiMaintenanceCandidateMembers;
use weather_contracts::*;

const REGISTRATION: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const CANDIDATE: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const BUNDLE: &str = "3333333333333333333333333333333333333333333333333333333333333333";
const SOURCE: &str = "4444444444444444444444444444444444444444444444444444444444444444";
const ACTION: &str = "5555555555555555555555555555555555555555555555555555555555555555";
const PARITY: &str = "6666666666666666666666666666666666666666666666666666666666666666";
const OBSERVED: &str = "7777777777777777777777777777777777777777777777777777777777777777";
const INSTALLED: &str = "2026-08-20T01:02:03.000Z";
const BEGINS: &str = "2026-09-01T00:00:00.000Z";
const ENDS: &str = "2026-10-01T00:00:00.000Z";
const READY: &str = "2026-10-02T00:00:00.000Z";
const TEMPERATURE_SOURCE_WIRE: &[u8] = b"{\"adapterVersion\":\"open-meteo-ecmwf-single-run/v1\",\"cohort\":\"ecmwf_single_run_hindcast\",\"dataset\":\"single_run\",\"maximumReceiptAgeHours\":12,\"providerKey\":\"open-meteo\",\"scope\":\"assumed_delay6_next12\",\"sourceDelayHours\":6,\"upstreamModel\":\"ecmwf_ifs\"}\n";
const TEMPERATURE_SOURCE_DIGEST: &str =
    "67e5f000e0eb2c56a68b7846922773fadefd776400975814838e3ff97e0723af";

#[derive(Default)]
struct Files {
    catalog_reads: usize,
    member_reads: usize,
}

impl ApiMaintenanceCandidateFiles for Files {
    /// Return the immutable root catalog receipt for each independent startup selection.
    fn read_catalog(&mut self) -> Result<Vec<u8>, ApiMaintenanceCandidateError> {
        self.catalog_reads += 1;
        Ok(b"catalog\n".to_vec())
    }

    /// The only installed slot supplies exactly its selected immutable member identities.
    fn read_members(
        &mut self,
        family: AdjustmentShadowFamily,
        action: &str,
        candidate: &str,
        bundle: &str,
    ) -> Result<ApiMaintenanceCandidateMembers, ApiMaintenanceCandidateError> {
        self.member_reads += 1;
        assert_eq!(
            (family, action, candidate, bundle),
            (
                AdjustmentShadowFamily::Temperature,
                ACTION,
                CANDIDATE,
                BUNDLE
            )
        );
        Ok(ApiMaintenanceCandidateMembers {
            action: b"action\n".to_vec(),
            projection: b"projection\n".to_vec(),
            parity: b"parity\n".to_vec(),
            bundle: b"bundle\n".to_vec(),
        })
    }
}

struct Values;

impl crate::MaintenanceCanonicalJson for Values {
    /// Provide typed fixture values at the approved parser boundary.
    fn parse_canonical_object(&mut self, bytes: &[u8]) -> Option<JsonValue> {
        match bytes {
            b"catalog\n" => Some(catalog()),
            b"action\n" => Some(action()),
            b"projection\n" => Some(projection()),
            b"parity\n" => Some(parity()),
            b"bundle\n" => Some(package_header()),
            _ => None,
        }
    }

    /// Observe exact registration and package hash preimages without implementing a JSON codec.
    fn encode_canonical_json(&mut self, value: &JsonValue) -> Option<Vec<u8>> {
        // projection comparison and the SQL bind share the same unchanged registration fields
        if value == &registration() {
            return Some(format!("{}\n", registration_wire()).into_bytes());
        }
        // preserve the original compiled Node source preimage
        if value == &temperature_source() {
            return Some(TEMPERATURE_SOURCE_WIRE.to_vec());
        }
        // the package identity must omit only its own hash field
        let JsonValue::Object(mut unsigned) = package_header() else {
            unreachable!()
        };
        unsigned.remove(0);
        assert_eq!(value, &JsonValue::Object(unsigned));
        Some(b"unsigned temperature\n".to_vec())
    }

    /// Keep registration identity tied to the exact v2 preimage and selected source.
    fn sha256(&mut self, bytes: &[u8]) -> Option<Sha256Digest> {
        let identity = match bytes {
            b"action\n" => ACTION,
            b"parity\n" => PARITY,
            b"unsigned temperature\n" => BUNDLE,
            TEMPERATURE_SOURCE_WIRE => TEMPERATURE_SOURCE_DIGEST,
            b"catalog\n" => OBSERVED,
            other => {
                let expected = [
                    "adjustment-shadow-registration/v2",
                    "ballydidean",
                    "temperature",
                    CANDIDATE,
                    BUNDLE,
                    OBSERVED,
                    OBSERVED,
                    OBSERVED,
                    SOURCE,
                    BEGINS,
                    ENDS,
                    READY,
                    READY,
                ]
                .join("\n");
                assert_eq!(other, expected.as_bytes());
                REGISTRATION
            }
        };
        Some(Sha256Digest::admitted(identity.to_owned()))
    }
}

impl ApiMaintenanceCandidateCodec for Values {
    /// Recognize exactly the fixture timestamps used by the retained startup path.
    fn is_canonical_utc_instant(&mut self, value: &str) -> bool {
        [INSTALLED, BEGINS, ENDS, READY].contains(&value)
    }

    /// Provide the already decoded generated package for the Rust model-shape verifier.
    fn decode_maintenance_package(
        &mut self,
        value: &JsonValue,
    ) -> Option<ForecastAdjustmentMaintenanceRuntimePackage> {
        assert_eq!(value, &package_header());
        let strength = TemperatureMosRuntimeStrengthBand {
            alpha: 0.5,
            supported: true,
            trainingCutoffUtc: INSTALLED.to_owned(),
        };
        Some(ForecastAdjustmentMaintenanceRuntimePackage::Temperature(
            ForecastAdjustmentTemperatureMaintenanceRuntimePackage {
                bundleSha256: BUNDLE.to_owned(),
                candidateSha256: CANDIDATE.to_owned(),
                contractVersion: ForecastAdjustmentTemperatureMaintenanceRuntimePackageContractVersionLiteral,
                dueMonth: "2026-09".to_owned(),
                family: ForecastAdjustmentTemperatureMaintenanceRuntimePackageFamilyLiteral,
                source: ForecastAdjustmentTemperatureMaintenanceRuntimePackageSourceObject {
                    adapterVersion: ForecastAdjustmentTemperatureMaintenanceRuntimePackageSourceAdapterVersionLiteral,
                    cohort: ForecastAdjustmentTemperatureMaintenanceRuntimePackageSourceCohortLiteral,
                    dataset: ForecastAdjustmentTemperatureMaintenanceRuntimePackageSourceDatasetLiteral,
                    maximumReceiptAgeHours: ForecastAdjustmentTemperatureMaintenanceRuntimePackageSourceMaximumReceiptAgeHoursLiteral,
                    providerKey: ForecastAdjustmentTemperatureMaintenanceRuntimePackageSourceProviderKeyLiteral,
                    scope: ForecastAdjustmentTemperatureMaintenanceRuntimePackageSourceScopeLiteral,
                    sourceDelayHours: ForecastAdjustmentTemperatureMaintenanceRuntimePackageSourceSourceDelayHoursLiteral,
                    upstreamModel: ForecastAdjustmentTemperatureMaintenanceRuntimePackageSourceUpstreamModelLiteral,
                },
                model: TemperatureMosPermanentRuntimeModel {
                    adaptiveCoefficients: Nullable::Value(vec![0.0; 49]),
                    directCoefficients: Nullable::Value(vec![0.0; 35]),
                    contractVersion: TemperatureMosPermanentRuntimeModelContractVersionLiteral,
                    cohort: TemperatureMosRuntimeModelMaterialCohortLiteral,
                    effectiveFrom: BEGINS.to_owned(),
                    latestTrainingValidAt: INSTALLED.to_owned(),
                    learnedStrengthContractVersion: TemperatureMosRuntimeModelMaterialLearnedStrengthContractVersionLiteral,
                    scope: TemperatureMosRuntimeModelMaterialScopeEnum::AssumedDelay6Next12,
                    strengthBands: TemperatureMosRuntimeModelMaterialStrengthBands { hours1To6: strength.clone(), hours7To12: strength },
                    supported: true,
                    trainingCutoffUtc: INSTALLED.to_owned(),
                },
            }
        ))
    }
}

impl crate::ApiMaintenanceReceiptCanonicalValues for Values {
    /// Reuse the retained fixture calendar without a new date parser.
    fn canonical_utc_epoch_milliseconds(&self, value: &str) -> Option<i64> {
        match value {
            INSTALLED => Some(1_787_187_723_000),
            BEGINS => Some(1_788_220_800_000),
            _ => None,
        }
    }

    /// Hash the same package preimages through the existing Node oracle.
    fn sha256(&self, bytes: &[u8]) -> Option<Sha256Digest> {
        crate::MaintenanceCanonicalJson::sha256(&mut Values, bytes)
    }

    /// Package validation does not produce an acknowledgement.
    fn unsigned_receipt_sha256(
        &self,
        _: &[(&str, crate::MaintenanceReceiptScalar<'_>)],
    ) -> Option<Sha256Digest> {
        unreachable!()
    }
}

impl crate::ApiMaintenanceShadowEvidenceValues for Values {
    /// Comparator package admission starts from the already decoded artifact.
    fn decode_shadow_json(&self, _: &[u8]) -> Option<JsonValue> {
        unreachable!()
    }

    /// Package identity uses sorted rather than input-ordered serialization.
    fn encode_shadow_ordered_json(&self, _: &JsonValue) -> Option<Vec<u8>> {
        unreachable!()
    }

    /// The fixture bypasses its preceding comparator envelope.
    fn decode_shadow_comparator_base64(&self, _: &[u16]) -> Option<Vec<u8>> {
        unreachable!()
    }

    /// Use the original package and source preimages in both paths.
    fn encode_shadow_sorted_json(&self, value: &JsonValue) -> Option<Vec<u8>> {
        crate::MaintenanceCanonicalJson::encode_canonical_json(&mut Values, value)
    }

    /// Borrow the same generated package view as installed startup.
    fn decode_shadow_maintenance_package(
        &self,
        value: &JsonValue,
    ) -> Option<ForecastAdjustmentMaintenanceRuntimePackage> {
        Values.decode_maintenance_package(value)
    }

    /// This fixture carries a temperature package rather than a portable rain artifact.
    fn decode_shadow_rain_artifact(
        &self,
        _: &JsonValue,
    ) -> Option<weather_contracts::RainHurdleWindPortableArtifact> {
        unreachable!()
    }

    /// This fixture never examines a general wind runtime string.
    fn check_legacy_wind_runtime_value(&self, _: &[u16]) -> Option<()> {
        unreachable!()
    }
}

/// Verify a qualified package by identity without requiring an installed catalog receipt.
#[test]
fn admits_qualified_comparator_package_using_the_same_model_owner() {
    use crate::api_maintenance_candidate_package::verify_comparator_maintenance_package;

    let package = package_header();
    assert!(verify_comparator_maintenance_package(
        &Values,
        &package,
        BUNDLE,
        AdjustmentShadowFamily::Temperature
    ));
    assert!(!verify_comparator_maintenance_package(
        &Values,
        &package,
        CANDIDATE,
        AdjustmentShadowFamily::Temperature
    ));
}

/// Join the original selected candidate and all four independently addressed members.
#[test]
fn loads_installed_temperature_and_omits_absent_wind() {
    let mut files = Files::default();
    let loaded = load_api_maintenance_candidates(&mut files, &mut Values);
    let temperature = loaded
        .temperature
        .as_ref()
        .expect("temperature candidate installed");
    assert_eq!(temperature.package.bundleSha256, BUNDLE);
    assert!(temperature.registration.has_capture_receipt_v1);
    let registration = temperature.registration.as_admitted();
    assert_eq!(registration.version, ApiMaintenanceRegistrationVersion::V2);
    assert_eq!(registration.registration_sha256, REGISTRATION);
    assert_eq!(registration.canonical_json, registration_wire());
    assert_eq!(
        loaded
            .catalog_sha256(AdjustmentShadowFamily::Temperature)
            .map(Sha256Digest::as_str),
        Some(OBSERVED)
    );
    assert!(loaded.wind.is_none());
    assert_eq!((files.catalog_reads, files.member_reads), (2, 1));
}

/// Build the original selected v1 catalog with one temperature shadow entry.
fn catalog() -> JsonValue {
    let receipt = strings(&[
        (
            "contractVersion",
            "adjustment-installed-candidate-receipt/v1",
        ),
        ("actionSha256", ACTION),
        ("registrationSha256", REGISTRATION),
        ("candidateGraphSha256", OBSERVED),
        ("candidateSha256", CANDIDATE),
        ("bundleSha256", BUNDLE),
        ("sourceSha256", SOURCE),
        ("installedAt", INSTALLED),
        ("deployedCommit", "abcdefabcdefabcdefabcdefabcdefabcdefabcd"),
        ("deployedRelease", "2026.08.20-1"),
        ("fencingToken", "1"),
        ("deployedImageDigest", &format!("sha256:{OBSERVED}")),
        ("deployedSettingsSha256", OBSERVED),
        ("controlPlaneSha256", OBSERVED),
    ]);
    let entry = object([
        ("family", string("temperature")),
        ("receipt", receipt),
        ("registration", registration()),
    ]);
    object([
        (
            "contractVersion",
            string("adjustment-installed-candidate-catalog/v1"),
        ),
        ("entries", JsonValue::Array(vec![entry])),
    ])
}

/// Build the closed v2 database registration object.
fn registration() -> JsonValue {
    strings(&[
        ("artifactSha256", BUNDLE),
        ("candidateSha256", CANDIDATE),
        ("cohortSha256", OBSERVED),
        ("family", "temperature"),
        ("intervalEndAt", ENDS),
        ("intervalStartAt", BEGINS),
        ("policySha256", OBSERVED),
        ("registrationSha256", REGISTRATION),
        ("reservedKeySha256", OBSERVED),
        ("siteKey", "ballydidean"),
        ("sourceSha256", SOURCE),
        ("targetCutoffAt", READY),
        ("terminalAt", READY),
    ])
}

/// Build the original inactive action with literal-null ancestry.
fn action() -> JsonValue {
    let JsonValue::Object(mut fields) = strings(&[
        ("actionKind", "shadow"),
        ("candidateGraphSha256", OBSERVED),
        ("candidateSha256", CANDIDATE),
        ("contractVersion", "forecast-adjustment-model-action/v1"),
        ("createdAt", INSTALLED),
        ("expectedSettingsSha256", OBSERVED),
        (
            "expectedSourceCommit",
            "abcdefabcdefabcdefabcdefabcdefabcdefabcd",
        ),
        ("expectedSourceRelease", "2026.08.20-1"),
        ("family", "temperature"),
        ("fencingToken", "1"),
        ("lifecycleHeadSha256", OBSERVED),
        ("policyDecision", "pending"),
        ("policyReportSha256", OBSERVED),
        ("reason", "development_candidate"),
        ("reportCreatedAt", INSTALLED),
        ("siteKey", "ballydidean"),
        ("validThrough", READY),
    ]) else {
        unreachable!()
    };
    fields.extend(
        [
            "expectedInstalledReceiptSha256",
            "fullMemberRootSha256",
            "predecessorActionSha256",
        ]
        .map(|key| (key.encode_utf16().collect(), JsonValue::Null)),
    );
    JsonValue::Object(fields)
}

/// Retain the same registration under the exact source-image projection.
fn projection() -> JsonValue {
    let JsonValue::Object(mut fields) = strings(&[
        ("actionSha256", ACTION),
        ("artifactSha256", BUNDLE),
        ("bundleSha256", BUNDLE),
        ("candidateGraphSha256", OBSERVED),
        ("candidateSha256", CANDIDATE),
        (
            "contractVersion",
            "forecast-adjustment-shadow-catalog-projection/v1",
        ),
        ("family", "temperature"),
        ("paritySha256", PARITY),
        ("siteKey", "ballydidean"),
    ]) else {
        unreachable!()
    };
    fields.push(("registration".encode_utf16().collect(), registration()));
    JsonValue::Object(fields)
}

/// Require identical retained and synthetic model outputs in the parity receipt.
fn parity() -> JsonValue {
    strings(&[
        ("candidateSha256", CANDIDATE),
        ("contractVersion", "forecast-adjustment-model-parity/v1"),
        ("family", "temperature"),
        ("retainedInputSha256", OBSERVED),
        ("retainedNativeOutputSha256", OBSERVED),
        ("retainedPackagedOutputSha256", OBSERVED),
        ("syntheticInputSha256", OBSERVED),
        ("syntheticNativeOutputSha256", OBSERVED),
        ("syntheticPackagedOutputSha256", OBSERVED),
    ])
}

/// Build the exact package envelope with body shapes admitted by the injected typed decoder.
fn package_header() -> JsonValue {
    object([
        ("bundleSha256", string(BUNDLE)),
        ("candidateSha256", string(CANDIDATE)),
        (
            "contractVersion",
            string("forecast-adjustment-maintenance-runtime-package/v1"),
        ),
        ("dueMonth", string("2026-09")),
        ("family", string("temperature")),
        ("model", temperature_model()),
        ("source", temperature_source()),
    ])
}

/// Construct fixture objects while preserving their original UTF-16 field spellings.
fn object<const N: usize>(fields: [(&str, JsonValue); N]) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(key, value)| (key.encode_utf16().collect(), value))
            .collect(),
    )
}

/// Construct a fixture scalar map without hand-authoring bytes.
fn strings(fields: &[(&str, &str)]) -> JsonValue {
    JsonValue::Object(
        fields
            .iter()
            .map(|(key, value)| (key.encode_utf16().collect(), string(value)))
            .collect(),
    )
}

/// Own an exact TypeScript fixture string.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Provide the selected fixed temperature source object in the decoded fixture.
fn temperature_source() -> JsonValue {
    object([
        ("adapterVersion", string("open-meteo-ecmwf-single-run/v1")),
        ("cohort", string("ecmwf_single_run_hindcast")),
        ("dataset", string("single_run")),
        ("maximumReceiptAgeHours", JsonValue::Number(12.0)),
        ("providerKey", string("open-meteo")),
        ("scope", string("assumed_delay6_next12")),
        ("sourceDelayHours", JsonValue::Number(6.0)),
        ("upstreamModel", string("ecmwf_ifs")),
    ])
}

/// Provide the complete admitted permanent model before typed decode.
fn temperature_model() -> JsonValue {
    let band = object([
        ("alpha", JsonValue::Number(0.5)),
        ("supported", JsonValue::Boolean(true)),
        ("trainingCutoffUtc", string(INSTALLED)),
    ]);
    object([
        (
            "adaptiveCoefficients",
            JsonValue::Array(vec![JsonValue::Number(0.0); 49]),
        ),
        ("cohort", string("ecmwf_single_run_hindcast")),
        ("contractVersion", string("temperature-permanent-model/v1")),
        (
            "directCoefficients",
            JsonValue::Array(vec![JsonValue::Number(0.0); 35]),
        ),
        ("effectiveFrom", string(BEGINS)),
        ("latestTrainingValidAt", string(INSTALLED)),
        (
            "learnedStrengthContractVersion",
            string("temperature-winner-extensions-research/v1"),
        ),
        ("scope", string("assumed_delay6_next12")),
        (
            "strengthBands",
            object([("1-6", band.clone()), ("7-12", band)]),
        ),
        ("supported", JsonValue::Boolean(true)),
        ("trainingCutoffUtc", string(INSTALLED)),
    ])
}

/// Return the exact sorted registration bytes from the fixture's approved encoder.
fn registration_wire() -> String {
    format!(
        concat!(
            r#"{{"artifactSha256":"{}","candidateSha256":"{}","cohortSha256":"{}","family":"temperature","#,
            r#""intervalEndAt":"{}","intervalStartAt":"{}","policySha256":"{}","registrationSha256":"{}","#,
            r#""reservedKeySha256":"{}","siteKey":"ballydidean","sourceSha256":"{}","targetCutoffAt":"{}","terminalAt":"{}"}}"#,
        ),
        BUNDLE,
        CANDIDATE,
        OBSERVED,
        ENDS,
        BEGINS,
        OBSERVED,
        REGISTRATION,
        OBSERVED,
        SOURCE,
        READY,
        READY
    )
}
