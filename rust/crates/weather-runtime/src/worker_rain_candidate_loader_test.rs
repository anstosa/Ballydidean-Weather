//! Functional coverage for the worker-owned installed rain candidate boundary.

use super::*;
use crate::{
    ApiMaintenanceCandidateFiles, ApiMaintenanceCandidateMembers,
    ApiMaintenanceReceiptCanonicalValues, ApiMaintenanceShadowEvidenceValues,
    ApiQualifiedCanaryFiles, ApiQualifiedCanaryMembers, MaintenanceCanonicalJson,
    MaintenanceReceiptScalar, RainMaintenanceCandidateFiles, WorkerRainCandidateMembers,
    WorkerRainControlMembers, WorkerRainControlReferenceFiles, WorkerRainControlSelectorMembers,
    WorkerRainServingFiles, WorkerRainServingMembers,
};
use weather_contracts::*;
use weather_database::{ApiMaintenanceRegistrationVersion, Sha256Digest};

const REGISTRATION: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const CANDIDATE: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const BUNDLE: &str = "3333333333333333333333333333333333333333333333333333333333333333";
const SOURCE: &str = "4444444444444444444444444444444444444444444444444444444444444444";
const ACTION: &str = "5555555555555555555555555555555555555555555555555555555555555555";
const PARITY: &str = "6666666666666666666666666666666666666666666666666666666666666666";
const OBSERVED: &str = "7777777777777777777777777777777777777777777777777777777777777777";
const COMPILED: &str = "8888888888888888888888888888888888888888888888888888888888888888";
const REGISTRY: &str = "9999999999999999999999999999999999999999999999999999999999999999";
const FEATURE_NAMES: &str = "6c8be26782bae556a152aa83ecf2f9dec2089561a635a20d6510f16a979a3650";
const INSTALLED: &str = "2026-08-20T01:02:03.000Z";
const BEGINS: &str = "2026-09-01T00:00:00.000Z";
const ENDS: &str = "2026-10-01T00:00:00.000Z";
const READY: &str = "2026-10-02T00:00:00.000Z";
const MODEL_MONTH: &str = "2026-09";
const TREE_COUNT: usize = 160;
const FEATURE_COUNT: usize = 107;

#[derive(Default)]
struct Files {
    catalog_reads: usize,
    candidate_reads: usize,
    damaged: bool,
    empty: bool,
}

impl ApiMaintenanceCandidateFiles for Files {
    /// Return the exact immutable catalog bytes selected by startup.
    fn read_catalog(&mut self) -> Result<Vec<u8>, ApiMaintenanceCandidateError> {
        self.catalog_reads += 1;
        // use the same file owner to observe omission without permitting member reads
        Ok(if self.empty {
            b"empty catalog\n".to_vec()
        } else {
            b"catalog\n".to_vec()
        })
    }

    /// Keep API-owned temperature and wind lookup outside this worker-only boundary.
    fn read_members(
        &mut self,
        _: AdjustmentShadowFamily,
        _: &str,
        _: &str,
        _: &str,
    ) -> Result<ApiMaintenanceCandidateMembers, ApiMaintenanceCandidateError> {
        unreachable!("worker rain loading must use rain custody")
    }
}

impl RainMaintenanceCandidateFiles for Files {
    /// Return all five rain members only for the identities selected by the catalog.
    fn read_rain_members(
        &mut self,
        action: &str,
        candidate: &str,
        bundle: &str,
    ) -> Result<WorkerRainCandidateMembers, ApiMaintenanceCandidateError> {
        self.candidate_reads += 1;
        assert_eq!((action, candidate, bundle), (ACTION, CANDIDATE, BUNDLE));
        Ok(WorkerRainCandidateMembers {
            members: ApiMaintenanceCandidateMembers {
                action: b"action\n".to_vec(),
                projection: b"projection\n".to_vec(),
                parity: b"parity\n".to_vec(),
                // expose malformed selected bytes without changing catalog identity
                bundle: if self.damaged {
                    b"damaged bundle\n".to_vec()
                } else {
                    b"bundle\n".to_vec()
                },
            },
            package: b"package\n".to_vec(),
        })
    }

    /// Legacy v1 technical receipts must never request fixed control members.
    fn read_rain_control_members(
        &mut self,
        _: &str,
        _: &str,
    ) -> Result<WorkerRainControlMembers, ApiMaintenanceCandidateError> {
        unreachable!("v1 packages carry no fixed controls")
    }
}

impl ApiQualifiedCanaryFiles for Files {
    /// exclude unselected api canary reads
    fn read_qualified_canary_members(
        &mut self,
        _: AdjustmentShadowFamily,
        _: &str,
        _: &str,
    ) -> Result<ApiQualifiedCanaryMembers, ApiMaintenanceCandidateError> {
        unreachable!("fixture catalog carries no qualified API canary")
    }
}

impl WorkerRainControlReferenceFiles for Files {
    /// exclude independent control selector reads
    fn read_worker_rain_control_selector(
        &mut self,
        _: &str,
    ) -> Result<WorkerRainControlSelectorMembers, ApiMaintenanceCandidateError> {
        unreachable!("fixture catalog carries no rain control reference")
    }
}

impl WorkerRainServingFiles for Files {
    /// exclude incumbent registry reads
    fn read_worker_rain_registry(&mut self) -> Result<Vec<u8>, ApiMaintenanceCandidateError> {
        unreachable!("catalog verification must not load the incumbent rain registry")
    }

    /// exclude qualified rain serving reads
    fn read_worker_rain_serving_members(
        &mut self,
        _: &str,
        _: &str,
        _: &str,
    ) -> Result<WorkerRainServingMembers, ApiMaintenanceCandidateError> {
        unreachable!("fixture catalog carries no qualified rain serving slot")
    }
}

struct Values;

impl MaintenanceCanonicalJson for Values {
    /// Decode each original-format fixture member at the injected JSON boundary.
    fn parse_canonical_object(&mut self, bytes: &[u8]) -> Option<JsonValue> {
        match bytes {
            b"catalog\n" => Some(catalog()),
            b"empty catalog\n" => Some(empty_catalog()),
            b"action\n" => Some(action()),
            b"projection\n" => Some(projection()),
            b"parity\n" => Some(parity()),
            b"package\n" => Some(package()),
            _ => None,
        }
    }

    /// Retain canonical registration equality and its exact database payload.
    fn encode_canonical_json(&mut self, value: &JsonValue) -> Option<Vec<u8>> {
        (value == &registration()).then(|| format!("{}\n", registration_wire()).into_bytes())
    }

    /// Supply exact content identities at the already approved hash boundary.
    fn sha256(&mut self, bytes: &[u8]) -> Option<Sha256Digest> {
        let identity = match bytes {
            b"action\n" => ACTION,
            b"parity\n" => PARITY,
            b"catalog\n" => OBSERVED,
            other if other == registration_preimage().as_bytes() => REGISTRATION,
            _ => return None,
        };
        Some(Sha256Digest::admitted(identity.to_owned()))
    }
}

impl ApiMaintenanceCandidateCodec for Values {
    /// Recognize only clocks carried by this installed-candidate fixture.
    fn is_canonical_utc_instant(&mut self, value: &str) -> bool {
        [INSTALLED, BEGINS, ENDS, READY].contains(&value)
    }

    /// Rain loading never routes its portable artifact through the API package union.
    fn decode_maintenance_package(
        &mut self,
        _: &JsonValue,
    ) -> Option<ForecastAdjustmentMaintenanceRuntimePackage> {
        unreachable!("rain has a worker-owned portable artifact decoder")
    }
}

impl ApiMaintenanceReceiptCanonicalValues for Values {
    /// No calendar arithmetic is needed by the installed rain candidate path.
    fn canonical_utc_epoch_milliseconds(&self, _: &str) -> Option<i64> {
        unreachable!("candidate loading validates clocks through its mutable codec")
    }

    /// Bind raw artifact and feature-name bytes to their original content identities.
    fn sha256(&self, bytes: &[u8]) -> Option<Sha256Digest> {
        let identity = match bytes {
            b"bundle\n" => BUNDLE,
            b"features\n" => FEATURE_NAMES,
            _ => return None,
        };
        Some(Sha256Digest::admitted(identity.to_owned()))
    }

    /// Candidate loading does not create an unsigned archive or database receipt.
    fn unsigned_receipt_sha256(
        &self,
        _: &[(&str, MaintenanceReceiptScalar<'_>)],
    ) -> Option<Sha256Digest> {
        unreachable!("candidate loading does not issue receipts")
    }
}

impl ApiMaintenanceShadowEvidenceValues for Values {
    /// Decode only the exact selected portable artifact bytes.
    fn decode_shadow_json(&self, bytes: &[u8]) -> Option<JsonValue> {
        (bytes == b"bundle\n").then(artifact_document)
    }

    /// Encode only the feature vector hashed by artifact admission.
    fn encode_shadow_ordered_json(&self, value: &JsonValue) -> Option<Vec<u8>> {
        (value == &feature_names_json()).then(|| b"features\n".to_vec())
    }

    /// This loader does not decode comparator base64 material.
    fn decode_shadow_comparator_base64(&self, _: &[u16]) -> Option<Vec<u8>> {
        unreachable!("candidate loading has no comparator")
    }

    /// Preserve the exact canonical selected artifact bytes.
    fn encode_shadow_sorted_json(&self, value: &JsonValue) -> Option<Vec<u8>> {
        (value == &artifact_document()).then(|| b"bundle\n".to_vec())
    }

    /// This loader does not decode the temperature or wind package union.
    fn decode_shadow_maintenance_package(
        &self,
        _: &JsonValue,
    ) -> Option<ForecastAdjustmentMaintenanceRuntimePackage> {
        unreachable!("rain has a portable artifact")
    }

    /// Return the typed model only for the matching closed portable document.
    fn decode_shadow_rain_artifact(
        &self,
        value: &JsonValue,
    ) -> Option<RainHurdleWindPortableArtifact> {
        (value == &artifact_document()).then(artifact)
    }

    /// General wind value compatibility is outside the rain portable model boundary.
    fn check_legacy_wind_runtime_value(&self, _: &[u16]) -> Option<()> {
        unreachable!("rain loading has no general wind runtime value")
    }
}

/// Prove a catalog-selected v1 model reaches a bound registration without control reads.
#[test]
fn loads_one_installed_v1_rain_candidate_and_registration() {
    let mut files = Files::default();
    let loaded = load_worker_rain_maintenance_candidate(&mut files, &mut Values, &Values)
        .expect("valid installed rain candidate")
        .expect("rain shadow slot selected");

    assert!(loaded.prepared().is_legacy_shadow_artifact());
    assert_eq!(loaded.catalog_sha256().as_str(), OBSERVED);
    let registration = loaded.registration();
    assert_eq!(registration.family, AdjustmentShadowFamily::Rain);
    assert_eq!(registration.version, ApiMaintenanceRegistrationVersion::V2);
    assert_eq!(registration.registration_sha256, REGISTRATION);
    assert_eq!(registration.candidate_sha256, CANDIDATE);
    assert_eq!(registration.canonical_json, registration_wire());
    assert_eq!((files.catalog_reads, files.candidate_reads), (1, 1));
}

/// Prove an omitted rain shadow slot performs no candidate or control reads.
#[test]
fn omits_rain_candidate_without_reading_members() {
    let mut files = Files {
        empty: true,
        ..Files::default()
    };
    let loaded = load_worker_rain_maintenance_candidate(&mut files, &mut Values, &Values)
        .expect("empty installed catalog remains valid");

    assert!(loaded.is_none());
    assert_eq!((files.catalog_reads, files.candidate_reads), (1, 0));
}

/// verify all seven original catalog reads
#[test]
fn verifies_all_catalog_roles_and_selected_rain_shadow_member() {
    let mut files = Files::default();

    crate::verify_installed_family_catalog(&mut files, &mut Values, &Values)
        .expect("valid installed family catalog");

    assert_eq!((files.catalog_reads, files.candidate_reads), (7, 1));
}

/// reject damaged selected rain bytes
#[test]
fn rejects_damaged_selected_rain_member_without_startup_fallback() {
    let mut files = Files {
        damaged: true,
        ..Files::default()
    };

    let error = crate::verify_installed_family_catalog(&mut files, &mut Values, &Values)
        .expect_err("damaged selected rain member must fail verification");

    assert_eq!(error, ApiMaintenanceCandidateError::Package);
    assert_eq!((files.catalog_reads, files.candidate_reads), (5, 1));
}

/// accept genuine absence across every role
#[test]
fn verifies_empty_catalog_without_reading_members() {
    let mut files = Files {
        empty: true,
        ..Files::default()
    };

    crate::verify_installed_family_catalog(&mut files, &mut Values, &Values)
        .expect("empty installed catalog remains valid");

    assert_eq!((files.catalog_reads, files.candidate_reads), (7, 0));
}

/// Build a valid legacy catalog with no installed family slots.
fn empty_catalog() -> JsonValue {
    object(vec![
        (
            "contractVersion",
            string("adjustment-installed-candidate-catalog/v1"),
        ),
        ("entries", JsonValue::Array(Vec::new())),
    ])
}

/// Build the selected legacy catalog with one rain shadow slot.
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
    object(vec![
        (
            "contractVersion",
            string("adjustment-installed-candidate-catalog/v1"),
        ),
        (
            "entries",
            JsonValue::Array(vec![object(vec![
                ("family", string("rain")),
                ("receipt", receipt),
                ("registration", registration()),
            ])]),
        ),
    ])
}

/// Build the exact v2 registration carried in the selected projection.
fn registration() -> JsonValue {
    strings(&[
        ("artifactSha256", BUNDLE),
        ("candidateSha256", CANDIDATE),
        ("cohortSha256", OBSERVED),
        ("family", "rain"),
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

/// Return the exact v2 registration hash preimage.
fn registration_preimage() -> String {
    [
        "adjustment-shadow-registration/v2",
        "ballydidean",
        "rain",
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
    .join("\n")
}

/// Return the canonical registration JSON observed by the database bind.
fn registration_wire() -> &'static str {
    "{\"artifactSha256\":\"3333333333333333333333333333333333333333333333333333333333333333\",\"candidateSha256\":\"2222222222222222222222222222222222222222222222222222222222222222\",\"cohortSha256\":\"7777777777777777777777777777777777777777777777777777777777777777\",\"family\":\"rain\",\"intervalEndAt\":\"2026-10-01T00:00:00.000Z\",\"intervalStartAt\":\"2026-09-01T00:00:00.000Z\",\"policySha256\":\"7777777777777777777777777777777777777777777777777777777777777777\",\"registrationSha256\":\"1111111111111111111111111111111111111111111111111111111111111111\",\"reservedKeySha256\":\"7777777777777777777777777777777777777777777777777777777777777777\",\"siteKey\":\"ballydidean\",\"sourceSha256\":\"4444444444444444444444444444444444444444444444444444444444444444\",\"targetCutoffAt\":\"2026-10-02T00:00:00.000Z\",\"terminalAt\":\"2026-10-02T00:00:00.000Z\"}"
}

/// Build the exact pending action selected by the receipt.
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
        ("family", "rain"),
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

/// Build the selected projection and nested registration.
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
        ("family", "rain"),
        ("paritySha256", PARITY),
        ("siteKey", "ballydidean"),
    ]) else {
        unreachable!()
    };
    fields.push(("registration".encode_utf16().collect(), registration()));
    JsonValue::Object(fields)
}

/// Build one native-to-package parity receipt with matching outputs.
fn parity() -> JsonValue {
    strings(&[
        ("candidateSha256", CANDIDATE),
        ("contractVersion", "forecast-adjustment-model-parity/v1"),
        ("family", "rain"),
        ("retainedInputSha256", OBSERVED),
        ("retainedNativeOutputSha256", OBSERVED),
        ("retainedPackagedOutputSha256", OBSERVED),
        ("syntheticInputSha256", OBSERVED),
        ("syntheticNativeOutputSha256", OBSERVED),
        ("syntheticPackagedOutputSha256", OBSERVED),
    ])
}

/// Build the v1 technical receipt that selects no control members.
fn package() -> JsonValue {
    strings(&[
        (
            "artifactPath",
            &format!("rain-runtime-artifacts/sha256-{BUNDLE}.json"),
        ),
        ("artifactSha256", BUNDLE),
        ("candidateSha256", CANDIDATE),
        (
            "compiledSourcePath",
            "packages/forecast-adjustment/src/rain-hurdle-wind-artifact.ts",
        ),
        ("compiledSourceSha256", COMPILED),
        (
            "contractVersion",
            "forecast-adjustment-rain-model-package/v1",
        ),
        ("modelMonth", MODEL_MONTH),
        ("registrySha256", REGISTRY),
    ])
}

/// Build one valid single-leaf native tree.
fn tree() -> [Vec<f64>; 5] {
    [vec![0.0], vec![0.0], vec![-1.0], vec![-1.0], vec![0.0]]
}

/// Build one exact 160-tree native head.
fn head(
    objective: RainHurdleWindPortableArtifactV1HeadsObjectiveEnum,
) -> RainHurdleWindPortableArtifactV1HeadsNativeHead {
    RainHurdleWindPortableArtifactV1HeadsNativeHead {
        objective,
        baseScore: 0.5,
        trees: (0..TREE_COUNT).map(|_| tree()).collect(),
    }
}

/// Build the typed v1 portable model returned by the injected decoder.
fn artifact() -> RainHurdleWindPortableArtifact {
    RainHurdleWindPortableArtifact::V1(RainHurdleWindPortableArtifactV1 {
        contractVersion: RainHurdleWindPortableArtifactV1ContractVersionLiteral,
        modelMonth: MODEL_MONTH.to_owned(),
        featureNames: feature_names(),
        rules: vec![
            RainHurdleWindPortableArtifactV1Rules {
                threshold: 0.1,
                cutoff: Nullable::Value(0.5),
            },
            RainHurdleWindPortableArtifactV1Rules {
                threshold: 1.0,
                cutoff: Nullable::Value(0.5),
            },
            RainHurdleWindPortableArtifactV1Rules {
                threshold: 2.5,
                cutoff: Nullable::Value(0.5),
            },
        ],
        categoryScales: vec![1.0, 1.0, 1.0],
        heads: RainHurdleWindPortableArtifactV1Heads {
            atLeast0_1: head(RainHurdleWindPortableArtifactV1HeadsObjectiveEnum::BinaryLogistic),
            atLeast1_0: head(RainHurdleWindPortableArtifactV1HeadsObjectiveEnum::BinaryLogistic),
            atLeast2_5: head(RainHurdleWindPortableArtifactV1HeadsObjectiveEnum::BinaryLogistic),
            amount: head(RainHurdleWindPortableArtifactV1HeadsObjectiveEnum::RegGamma),
        },
    })
}

/// Build the closed raw v1 portable document admitted before typed decode.
fn artifact_document() -> JsonValue {
    object(vec![
        (
            "categoryScales",
            JsonValue::Array(vec![JsonValue::Number(1.0); 3]),
        ),
        ("contractVersion", string("rain-hurdle-wind-runtime/v1")),
        ("featureNames", feature_names_json()),
        (
            "heads",
            object(vec![
                ("0.1", head_document("binary:logistic")),
                ("1.0", head_document("binary:logistic")),
                ("2.5", head_document("binary:logistic")),
                ("amount", head_document("reg:gamma")),
            ]),
        ),
        ("modelMonth", string(MODEL_MONTH)),
        (
            "rules",
            JsonValue::Array(vec![0.1, 1.0, 2.5].into_iter().map(rule_document).collect()),
        ),
    ])
}

/// Build one raw rule with its threshold and cutoff.
fn rule_document(threshold: f64) -> JsonValue {
    object(vec![
        ("cutoff", JsonValue::Number(0.5)),
        ("threshold", JsonValue::Number(threshold)),
    ])
}

/// Build one raw head with the frozen tree cardinality.
fn head_document(objective: &str) -> JsonValue {
    object(vec![
        ("baseScore", JsonValue::Number(0.5)),
        ("objective", string(objective)),
        (
            "trees",
            JsonValue::Array((0..TREE_COUNT).map(|_| tree_document()).collect()),
        ),
    ])
}

/// Build one raw single-leaf tree.
fn tree_document() -> JsonValue {
    JsonValue::Array(vec![
        JsonValue::Array(vec![JsonValue::Number(0.0)]),
        JsonValue::Array(vec![JsonValue::Number(0.0)]),
        JsonValue::Array(vec![JsonValue::Number(-1.0)]),
        JsonValue::Array(vec![JsonValue::Number(-1.0)]),
        JsonValue::Array(vec![JsonValue::Number(0.0)]),
    ])
}

/// Build the frozen feature names in typed form.
fn feature_names() -> Vec<String> {
    (0..FEATURE_COUNT)
        .map(|index| format!("feature_{index}"))
        .collect()
}

/// Build the frozen feature names in raw JSON form.
fn feature_names_json() -> JsonValue {
    JsonValue::Array(
        feature_names()
            .into_iter()
            .map(|name| string(&name))
            .collect(),
    )
}

/// Build one dependency-free JSON object.
fn object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(key, value)| (key.encode_utf16().collect(), value))
            .collect(),
    )
}

/// Build one dependency-free string map.
fn strings(fields: &[(&str, &str)]) -> JsonValue {
    JsonValue::Object(
        fields
            .iter()
            .map(|(key, value)| (key.encode_utf16().collect(), string(value)))
            .collect(),
    )
}

/// Build one dependency-free JSON string.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}
