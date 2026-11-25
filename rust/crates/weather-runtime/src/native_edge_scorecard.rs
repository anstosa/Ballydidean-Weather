//! Native aggregate scorecard file admission and response serialization.

use std::fmt::Write as _;

use weather_contracts::JsonValue;
use weather_edge::{
    AdmittedForecastAdjustmentScorecardJson, AdmittedForecastAdjustmentScorecardNow,
    AdmittedForecastAdjustmentScorecardResponseJson, AdmittedForecastAdjustmentScorecardSnapshot,
    ArchiveWirePrimitives, ForecastAdjustmentScorecardContractVersion,
    ForecastAdjustmentScorecardEncoder, ForecastAdjustmentScorecardFileAdapter,
    ForecastAdjustmentScorecardPublicationState, RevisionArchiveCanonicalCodec,
};

use crate::{
    FORECAST_SCORECARD_V1, FORECAST_SCORECARD_V2, NativeWirePrimitives, ScorecardContractError,
    ScorecardContractValues,
    maintenance_canonical_json::{exact_keys, text, valid_hash},
    parse_forecast_adjustment_scorecard,
};

/// Source scorecard JSON failure using the existing contract diagnostic.
fn invalid(message: &str) -> ScorecardContractError {
    ScorecardContractError::new(message)
}

impl ScorecardContractValues for NativeWirePrimitives {
    /// Decode the existing UTF-8 scorecard document through the retained native JSON owner.
    fn decode_scorecard_json(&mut self, bytes: &[u8]) -> Result<JsonValue, ScorecardContractError> {
        self.decode_json(String::from_utf8_lossy(bytes).as_bytes())
            .ok_or_else(|| invalid("forecast adjustment scorecard is invalid"))
    }

    /// Preserve the source's fixed case-insensitive private-member policy.
    fn scorecard_private_member_key(&self, key: &[u16]) -> bool {
        // match the fixed ASCII literals using Unicode simple folding for applicable letters
        let folded: String = String::from_utf16_lossy(key)
            .chars()
            .map(|character| match character {
                '\u{017f}' => 's',
                _ => character.to_ascii_lowercase(),
            })
            .collect();
        if folded.contains("private") {
            return true;
        }
        // JavaScript dot excludes its four line terminators even in Unicode mode
        folded
            .split(['\r', '\n', '\u{2028}', '\u{2029}'])
            .any(|segment| {
                segment
                    .split_once("reserved")
                    .is_some_and(|(_, rest)| rest.contains("member"))
                    || segment
                        .split_once("member")
                        .is_some_and(|(_, rest)| rest.contains("result"))
            })
    }
}

/// Native exact scorecard content reader and compact aggregate encoder.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeEdgeScorecardAdapter;

impl ForecastAdjustmentScorecardFileAdapter for NativeEdgeScorecardAdapter {
    type Error = ScorecardContractError;

    /// Decode the exact hash-only source pointer document.
    fn decode_pointer(&self, content: &[u8]) -> Result<String, Self::Error> {
        let value = NativeWirePrimitives
            .decode_json(String::from_utf8_lossy(content).as_bytes())
            .ok_or_else(|| invalid("scorecard pointer is invalid"))?;
        // retain the exact one-member pointer shape before admitting its digest
        if !exact_keys(&value, &["sha256"]) {
            return Err(invalid("scorecard pointer is invalid"));
        }
        text(&value, "sha256")
            .filter(|digest| valid_hash(digest))
            .ok_or_else(|| invalid("scorecard pointer is invalid"))
    }

    /// Hash the exact selected immutable object with the existing approved digest owner.
    fn sha256_lower_hex(&self, content: &[u8]) -> Result<String, Self::Error> {
        let digest = RevisionArchiveCanonicalCodec::sha256(&NativeWirePrimitives, content);
        let mut output = String::with_capacity(64);
        // render lowercase hexadecimal directly into the fixed-size output
        for byte in digest {
            write!(&mut output, "{byte:02x}").expect("string writes cannot fail");
        }
        Ok(output)
    }

    /// Validate both source aggregate contracts and retain their admitted epoch boundaries.
    fn decode_scorecard(
        &self,
        expected_version: ForecastAdjustmentScorecardContractVersion,
        content: &[u8],
        now: &AdmittedForecastAdjustmentScorecardNow,
    ) -> Result<AdmittedForecastAdjustmentScorecardSnapshot, Self::Error> {
        let value = parse_forecast_adjustment_scorecard(
            content,
            Some(now.epoch_ms),
            &mut NativeWirePrimitives,
        )?;
        let version = match expected_version {
            ForecastAdjustmentScorecardContractVersion::V1 => FORECAST_SCORECARD_V1,
            ForecastAdjustmentScorecardContractVersion::V2 => FORECAST_SCORECARD_V2,
        };
        // bind the parsed aggregate to the only permitted selected publication generation
        if text(&value, "contractVersion").as_deref() != Some(version) {
            return Err(invalid(
                "scorecard generation does not match its publication slot",
            ));
        }
        let generated_at = text(&value, "generatedAt")
            .and_then(|value| NativeWirePrimitives.canonical_utc_milliseconds(&value))
            .ok_or_else(|| invalid("scorecard generation time is invalid"))?;
        let valid_through = text(&value, "validThrough")
            .and_then(|value| NativeWirePrimitives.canonical_utc_milliseconds(&value))
            .ok_or_else(|| invalid("scorecard validity time is invalid"))?;
        let mut body = NativeWirePrimitives
            .encode_ordered_json(&value)
            .ok_or_else(|| invalid("scorecard response encoding failed"))?;
        // retain the compact source aggregate without the outer sendJson frame
        if body.pop() != Some(b'\n') {
            return Err(invalid("scorecard response encoding failed"));
        }
        Ok(AdmittedForecastAdjustmentScorecardSnapshot::admitted(
            expected_version,
            generated_at,
            AdmittedForecastAdjustmentScorecardJson::admitted(body),
            valid_through,
        ))
    }
}

impl ForecastAdjustmentScorecardEncoder for NativeEdgeScorecardAdapter {
    type Error = ScorecardContractError;

    /// Wrap the already admitted aggregate without decoding or serializing the nested object again.
    fn encode_response(
        &self,
        snapshot: &AdmittedForecastAdjustmentScorecardSnapshot,
        publication_state: ForecastAdjustmentScorecardPublicationState,
    ) -> Result<AdmittedForecastAdjustmentScorecardResponseJson, Self::Error> {
        let data = snapshot.scorecard().as_bytes();
        let mut body = Vec::with_capacity(data.len().saturating_add(48));
        body.extend_from_slice(b"{\"data\":");
        body.extend_from_slice(data);
        body.extend_from_slice(b",\"publicationState\":\"");
        body.extend_from_slice(publication_state.wire_value().as_bytes());
        body.extend_from_slice(b"\"}");
        Ok(AdmittedForecastAdjustmentScorecardResponseJson::admitted(
            body,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forecast_scorecard_cli::tests::ordinary_family_card;
    use crate::forecast_scorecard_contract::wire;

    /// Build a complete synthetic aggregate using existing canonical family fixtures.
    fn aggregate() -> JsonValue {
        let digest = wire(&"a".repeat(64));
        object(vec![
            ("contractVersion", wire(FORECAST_SCORECARD_V1)),
            ("generatedAt", wire("2026-10-08T00:00:00.000Z")),
            ("validThrough", wire("2026-10-15T00:00:00.000Z")),
            ("siteKey", wire("ballydidean")),
            ("servingChanged", JsonValue::Boolean(false)),
            ("automaticActivationEligible", JsonValue::Boolean(false)),
            ("operatorApprovalRequired", JsonValue::Boolean(true)),
            (
                "families",
                object(vec![
                    ("temperature", ordinary_family_card("temperature")),
                    ("wind", ordinary_family_card("wind")),
                    ("rain", ordinary_family_card("rain")),
                ]),
            ),
            (
                "inputs",
                object(vec![
                    ("adjustmentEvidenceManifestSha256", digest.clone()),
                    ("adjustmentEvidenceWatermarkSha256", digest.clone()),
                    ("forecastTrainingManifestSha256", digest.clone()),
                    ("localDateFrom", wire("2026-09-01")),
                    ("localDateTo", wire("2026-10-02")),
                    (
                        "reportSha256s",
                        object(vec![
                            ("temperature", digest.clone()),
                            ("wind", digest.clone()),
                            ("rain", digest),
                        ]),
                    ),
                    ("sourceRevision", wire(&"b".repeat(40))),
                    ("targetCutoffAt", wire("2026-10-02T07:00:00.000Z")),
                ]),
            ),
        ])
    }

    /// Retain the existing aggregate and pointer member order in the compact edge frame.
    #[test]
    fn native_edge_scorecard_accepts_and_frames_complete_v1_aggregate() {
        let adapter = NativeEdgeScorecardAdapter;
        let value = aggregate();
        let content = NativeWirePrimitives.encode_ordered_json(&value).unwrap();
        let digest = adapter.sha256_lower_hex(&content).unwrap();
        assert_eq!(
            adapter
                .decode_pointer(format!("{{\"sha256\":\"{digest}\"}}\n").as_bytes())
                .unwrap(),
            digest
        );
        let canonical = "2026-10-09T00:00:00.000Z".to_owned();
        let now = AdmittedForecastAdjustmentScorecardNow {
            epoch_ms: NativeWirePrimitives
                .canonical_utc_milliseconds(&canonical)
                .unwrap(),
            canonical,
        };
        let snapshot = adapter
            .decode_scorecard(
                ForecastAdjustmentScorecardContractVersion::V1,
                &content,
                &now,
            )
            .unwrap();
        let output = adapter
            .encode_response(
                &snapshot,
                ForecastAdjustmentScorecardPublicationState::LegacyDisplay,
            )
            .unwrap();
        let expected = object(vec![
            ("data", value),
            ("publicationState", wire("legacy_display")),
        ]);
        let mut expected = NativeWirePrimitives.encode_ordered_json(&expected).unwrap();
        expected.pop();
        assert_eq!(output.as_bytes(), expected);
    }

    /// Construct the fixture's ordered object members.
    fn object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
        JsonValue::Object(
            fields
                .into_iter()
                .map(|(key, value)| (key.encode_utf16().collect(), value))
                .collect(),
        )
    }
}
