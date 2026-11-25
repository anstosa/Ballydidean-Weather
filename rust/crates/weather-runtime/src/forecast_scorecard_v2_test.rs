use weather_contracts::JsonValue;

use super::validate_scorecard_v2;
use crate::forecast_scorecard_cli::tests::ordinary_family_card;
use crate::forecast_scorecard_contract::wire;
use crate::forecast_scorecard_fields::ScorecardClock;

const GENERATED_AT: &str = "2026-11-24T00:00:00.000Z";
const VALID_THROUGH: &str = "2026-12-01T00:00:00.000Z";
const EVIDENCE_CUTOFF: &str = "2026-10-07T00:00:00.000Z";
const SEVEN_DAYS_MS: i64 = 7 * 24 * 60 * 60 * 1_000;

struct FixtureClock;

impl ScorecardClock for FixtureClock {
    /// map the fixture instants without a runtime parser
    fn utc_epoch_milliseconds(&self, value: &str) -> Option<i64> {
        // select one admitted fixture instant
        match value {
            EVIDENCE_CUTOFF => Some(-1),
            GENERATED_AT => Some(0),
            VALID_THROUGH => Some(SEVEN_DAYS_MS),
            _ => None,
        }
    }

    /// restore the exact fixture spelling
    fn canonical_utc(&self, milliseconds: i64) -> Option<String> {
        // select one admitted fixture spelling
        match milliseconds {
            -1 => Some(EVIDENCE_CUTOFF.to_owned()),
            0 => Some(GENERATED_AT.to_owned()),
            SEVEN_DAYS_MS => Some(VALID_THROUGH.to_owned()),
            _ => None,
        }
    }
}

/// admit one complete v2 projection at the validity boundary
#[test]
fn admits_complete_v2_projection() {
    let digest = digest();
    let nullable_identities = object(vec![
        ("rain", JsonValue::Null),
        ("temperature", JsonValue::Null),
        ("wind", JsonValue::Null),
    ]);
    let raw_identities = object(vec![
        ("rain", digest.clone()),
        ("temperature", digest.clone()),
        ("wind", digest.clone()),
    ]);
    let scorecard = object(vec![
        (
            "actionLineage",
            object(vec![
                ("actionSha256", digest.clone()),
                ("attemptSha256", digest.clone()),
                ("predecessorActionSha256", JsonValue::Null),
                ("releaseManifestSha256", JsonValue::Null),
                ("sourceRevision", wire(&"a".repeat(40))),
            ]),
        ),
        ("actionProjectionSha256", digest.clone()),
        ("actionState", wire("none")),
        ("contractVersion", wire("forecast-adjustment-scorecard/v2")),
        (
            "families",
            object(vec![
                ("rain", ordinary_family_card("rain")),
                ("temperature", ordinary_family_card("temperature")),
                ("wind", ordinary_family_card("wind")),
            ]),
        ),
        ("generatedAt", wire(GENERATED_AT)),
        (
            "history",
            JsonValue::Array(vec![object(vec![
                ("actionLineageSha256", digest.clone()),
                ("actionProjectionSha256", digest.clone()),
                ("actionState", wire("none")),
                ("attemptSha256", digest.clone()),
                ("occurredAt", wire(GENERATED_AT)),
                ("policyDecision", wire("pending")),
                ("releaseManifestSha256", JsonValue::Null),
            ])]),
        ),
        (
            "identities",
            object(vec![
                ("active", nullable_identities.clone()),
                ("prior", nullable_identities.clone()),
                ("raw", raw_identities),
                ("shadow", nullable_identities),
            ]),
        ),
        (
            "inputs",
            object(vec![
                ("adjustmentEvidenceManifestSha256", digest.clone()),
                ("adjustmentEvidenceWatermarkSha256", digest.clone()),
                ("forecastTrainingManifestSha256", digest.clone()),
                ("frontierSha256", digest.clone()),
                ("inputManifestSha256", digest.clone()),
                ("localDateFrom", wire("2026-11-01")),
                ("localDateTo", wire("2026-11-24")),
                ("reportSha256", digest.clone()),
                (
                    "reportSha256s",
                    object(vec![
                        ("rain", digest.clone()),
                        ("temperature", digest.clone()),
                        ("wind", digest.clone()),
                    ]),
                ),
                ("sourceRevision", wire(&"a".repeat(40))),
                ("targetCutoffAt", wire(GENERATED_AT)),
            ]),
        ),
        (
            "job",
            object(vec![
                ("attemptState", wire("not_due")),
                ("backlogState", wire("clear")),
                ("dueState", wire("not_due")),
                ("operatorState", wire("enabled")),
                ("successState", wire("none")),
            ]),
        ),
        ("policyDecision", wire("pending")),
        (
            "progress",
            object(vec![
                ("confirmationCompletedEpochs", JsonValue::Number(0.0)),
                ("confirmationRequiredEpochs", JsonValue::Number(1.0)),
                ("rainCaptureExpiresAt", wire(VALID_THROUGH)),
                ("rollbackCompletedEpochs", JsonValue::Number(0.0)),
                ("rollbackRequiredEpochs", JsonValue::Number(1.0)),
            ]),
        ),
        ("siteKey", wire("ballydidean")),
        ("validThrough", wire(VALID_THROUGH)),
        (
            "warnings",
            object(vec![
                ("capacity", JsonValue::Array(Vec::new())),
                ("capture", JsonValue::Array(Vec::new())),
                ("fallback", JsonValue::Array(Vec::new())),
                ("gauge", JsonValue::Array(Vec::new())),
                ("source", JsonValue::Array(Vec::new())),
            ]),
        ),
    ]);

    assert_eq!(
        validate_scorecard_v2(&scorecard, Some(0), &FixtureClock),
        Ok(())
    );

    // exercise the installed native document validator with the real clock and exact input bytes
    use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};
    let wire = crate::NativeWirePrimitives;
    let now = wire
        .canonical_utc_milliseconds(GENERATED_AT)
        .expect("fixture timestamp");
    let encoded = wire.encode_ordered_json(&scorecard).expect("fixture wire");
    let expected =
        crate::native_maintenance_retirement_files_v2::format_digest(wire.sha256(&encoded));
    let mut output = Vec::new();
    crate::project_native_scorecard_install_document(
        encoded.as_slice(),
        &mut output,
        "v2",
        &expected,
        now,
    )
    .expect("native installation admission");
    assert_eq!(output, encoded);
}

/// construct one immutable digest fixture
fn digest() -> JsonValue {
    wire(&"a".repeat(64))
}

/// construct one json object without changing field values
fn object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
    // retain the caller's field population
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(key, value)| (key.encode_utf16().collect(), value))
            .collect(),
    )
}
