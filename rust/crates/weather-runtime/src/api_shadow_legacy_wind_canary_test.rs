//! Exercise the full native legacy wind canary verifier with fixed codec doubles.

use weather_contracts::{
    ForecastAdjustmentMaintenanceRuntimePackage, JsonValue, RainHurdleWindPortableArtifact,
};
use weather_database::Sha256Digest;

use super::api_shadow_legacy_wind_canary::verify_legacy_wind_canary_bundle;
use crate::maintenance_canonical_json::field;
use crate::{
    ApiMaintenanceReceiptCanonicalValues, ApiMaintenanceShadowEvidenceValues,
    MaintenanceReceiptScalar,
};

const BUNDLE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const FIT: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const PAYLOAD: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const REPORT: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const AUTHORIZATION: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const CUTOFF: &str = "2026-06-01T00:00:00.000Z";
const START: &str = "2026-06-02T00:00:00.000Z";
const END: &str = "2026-06-03T00:00:00.000Z";

struct Values(Vec<(Vec<u8>, &'static str)>);

impl ApiMaintenanceReceiptCanonicalValues for Values {
    /// Recognize the exact fixture's UTC clocks in chronological order.
    fn canonical_utc_epoch_milliseconds(&self, value: &str) -> Option<i64> {
        match value {
            CUTOFF => Some(0),
            START => Some(86_400_000),
            END => Some(172_800_000),
            _ => None,
        }
    }

    /// Admit only the expected material for each immutable artifact address.
    fn sha256(&self, bytes: &[u8]) -> Option<Sha256Digest> {
        let (_, expected) = self.0.iter().find(|(preimage, _)| preimage == bytes)?;
        Some(Sha256Digest::admitted(*expected))
    }

    /// This direct verifier does not produce new database receipts.
    fn unsigned_receipt_sha256(
        &self,
        _: &[(&str, MaintenanceReceiptScalar<'_>)],
    ) -> Option<Sha256Digest> {
        unreachable!()
    }
}

impl ApiMaintenanceShadowEvidenceValues for Values {
    /// Supply the already decoded bundle directly.
    fn decode_shadow_json(&self, _: &[u8]) -> Option<JsonValue> {
        unreachable!()
    }
    /// Supply no original wire envelope.
    fn encode_shadow_ordered_json(&self, _: &JsonValue) -> Option<Vec<u8>> {
        unreachable!()
    }
    /// Supply no embedded envelope.
    fn decode_shadow_comparator_base64(&self, _: &[u16]) -> Option<Vec<u8>> {
        unreachable!()
    }
    /// Use fixed test-only bytes that preserve structural equality of the closed fixture.
    fn encode_shadow_sorted_json(&self, doc: &JsonValue) -> Option<Vec<u8>> {
        Some(fixture_bytes(doc))
    }
    /// A canary bundle is not a qualified package.
    fn decode_shadow_maintenance_package(
        &self,
        _: &JsonValue,
    ) -> Option<ForecastAdjustmentMaintenanceRuntimePackage> {
        unreachable!()
    }
    /// A wind canary does not carry a portable rain artifact.
    fn decode_shadow_rain_artifact(&self, _: &JsonValue) -> Option<RainHurdleWindPortableArtifact> {
        unreachable!()
    }
    /// This fixture never examines a general wind runtime string.
    fn check_legacy_wind_runtime_value(&self, _: &[u16]) -> Option<()> {
        unreachable!()
    }
}

/// Encode a closed fixture deterministically without implementing a JSON codec.
fn fixture_bytes(value: &JsonValue) -> Vec<u8> {
    format!("{value:?}").into_bytes()
}

/// Build a decoded object at the approved JSON boundary.
fn object<const N: usize>(fields: [(&str, JsonValue); N]) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(key, value)| (key.encode_utf16().collect(), value))
            .collect(),
    )
}

/// Preserve original UTF-16 strings.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Return the original decoded member with its own hash omitted.
fn unsigned_fixture(node: &JsonValue, omitted: &str) -> Vec<u8> {
    let JsonValue::Object(fields) = node else {
        panic!("expected object fixture")
    };
    fixture_bytes(&JsonValue::Object(
        fields
            .iter()
            .filter(|(key, _)| !key.iter().copied().eq(omitted.encode_utf16()))
            .cloned()
            .collect(),
    ))
}

/// Retain one wind speed band, one fitted root, and its scalar envelope.
fn candidate(event_count: f64) -> JsonValue {
    let band = object([
        ("leadBand", string("001-024")),
        ("metric", string("windSpeedMps")),
    ]);
    object([
        (
            "algorithmContractVersion",
            string("robust-hierarchical-median/v1"),
        ),
        ("artifactKind", string("wind_transfer_canary_candidate")),
        ("candidateArtifactSha256", string(FIT)),
        ("coefficientPayloadSha256", string(PAYLOAD)),
        (
            "coefficients",
            JsonValue::Array(vec![object([
                ("coefficient", JsonValue::Number(1.0)),
                ("daypart", JsonValue::Null),
                ("effectiveEventCount", JsonValue::Number(event_count)),
                ("leadBand", string("001-024")),
                ("level", JsonValue::Number(1.0)),
                ("metric", string("windSpeedMps")),
                ("month", JsonValue::Null),
                ("season", JsonValue::Null),
            ])]),
        ),
        (
            "contractVersion",
            string("forecast-adjustment-wind-canary-candidate/v1"),
        ),
        ("enabledMetricBands", JsonValue::Array(vec![band])),
        ("exportManifestSha256", string(FIT)),
        ("finalTrainingCutoff", string(CUTOFF)),
        (
            "runtimeFingerprint",
            object([
                ("icuVersion", string("78")),
                ("tzdataVersion", string("2026a")),
            ]),
        ),
        (
            "servedForecastIdentity",
            object([("fixture", string("serving"))]),
        ),
        ("siteKey", string("ballydidean")),
        ("timezone", string("America/Los_Angeles")),
        (
            "trainingEnvelopes",
            JsonValue::Array(vec![object([
                ("leadBand", string("001-024")),
                ("maximum", JsonValue::Number(12.0)),
                ("metric", string("windSpeedMps")),
                ("minimum", JsonValue::Number(1.0)),
            ])]),
        ),
        (
            "trainingForecastIdentity",
            object([("fixture", string("training"))]),
        ),
        (
            "trainingProvenance",
            object([("fixture", string("provenance"))]),
        ),
    ])
}

/// Construct one permanent canary linked to a positive bridge report.
fn bundle(event_count: f64) -> (JsonValue, Values) {
    let fit = candidate(event_count);
    let bands = field(&fit, "enabledMetricBands").unwrap().clone();
    let JsonValue::Array(band_items) = &bands else {
        panic!("expected enabled bands")
    };
    let report = object([
        (
            "artifactKind",
            string("wind_transfer_canary_transfer_report"),
        ),
        ("bridgeEndExclusive", string(END)),
        (
            "bridgeEvaluations",
            JsonValue::Array(vec![object([
                ("metricBand", band_items[0].clone()),
                (
                    "network",
                    object([
                        ("adjustedLoss", JsonValue::Number(1.0)),
                        ("eventCount", JsonValue::Number(30.0)),
                        ("rawLoss", JsonValue::Number(2.0)),
                        ("skill", JsonValue::Number(0.5)),
                    ]),
                ),
            ])]),
        ),
        ("bridgeStartInclusive", string(START)),
        ("candidateArtifactSha256", string(FIT)),
        (
            "contractVersion",
            string("forecast-adjustment-wind-canary-transfer-report/v1"),
        ),
        ("enabledMetricBands", bands.clone()),
        ("passed", JsonValue::Boolean(true)),
        (
            "servedForecastIdentity",
            field(&fit, "servedForecastIdentity").unwrap().clone(),
        ),
        (
            "trainingForecastIdentity",
            field(&fit, "trainingForecastIdentity").unwrap().clone(),
        ),
        ("transferReportSha256", string(REPORT)),
    ]);
    let authorization = object([
        ("activatedAt", string(END)),
        ("artifactKind", string("wind_transfer_canary_authorization")),
        ("authorizationReason", string("verified transfer")),
        ("authorizationSha256", string(AUTHORIZATION)),
        ("authorized", JsonValue::Boolean(true)),
        ("authorizedAt", string(END)),
        ("authorizedBy", string("weather operator")),
        ("candidateArtifactSha256", string(FIT)),
        (
            "contractVersion",
            string("forecast-adjustment-wind-canary-authorization/v2"),
        ),
        ("enabledMetricBands", bands),
        ("expiresAt", JsonValue::Null),
        ("permanent", JsonValue::Boolean(true)),
        ("transferReportSha256", string(REPORT)),
    ]);
    let mut hashes = vec![
        (unsigned_fixture(&fit, "candidateArtifactSha256"), FIT),
        (fixture_bytes(field(&fit, "coefficients").unwrap()), PAYLOAD),
        (unsigned_fixture(&report, "transferReportSha256"), REPORT),
        (
            unsigned_fixture(&authorization, "authorizationSha256"),
            AUTHORIZATION,
        ),
    ];
    // bind the three original canonical lineage digests to distinct fixture documents
    for (key, digest) in [
        (
            "trainingForecastIdentity",
            "4cc285ce09bd743cc5d3b24c55722a9b0e68083f7b51c7009906b31d51b6cf15",
        ),
        (
            "servedForecastIdentity",
            "8348b5c48d7fe614ced9442b04e8a68cfe704e3d541eab3b8d0ab33a41cd4079",
        ),
        (
            "trainingProvenance",
            "cfa905468fb15fb10b3abf02a39db5823d8d3bb17c6dddc625749eb9fe3bb8c3",
        ),
    ] {
        hashes.push((fixture_bytes(field(&fit, key).unwrap()), digest));
    }
    let document = object([
        (
            "artifactKind",
            string("wind_transfer_canary_runtime_bundle"),
        ),
        ("authorization", authorization),
        ("bundleSha256", string(BUNDLE)),
        ("candidate", fit),
        (
            "contractVersion",
            string("forecast-adjustment-wind-canary-runtime-bundle/v2"),
        ),
        ("siteKey", string("ballydidean")),
        ("timezone", string("America/Los_Angeles")),
        ("transferReport", report),
    ]);
    hashes.push((unsigned_fixture(&document, "bundleSha256"), BUNDLE));
    (document, Values(hashes))
}

/// Accept the linked retained v2 path and refuse material below the original root sample floor.
#[test]
fn verifies_closed_wind_canary_and_requires_the_original_root_evidence() {
    let (document, values) = bundle(200.0);
    assert!(verify_legacy_wind_canary_bundle(&values, &document, BUNDLE).is_some());
    let (document, values) = bundle(199.0);
    assert!(verify_legacy_wind_canary_bundle(&values, &document, BUNDLE).is_none());
}
