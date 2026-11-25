//! Native admission for both original public forecast-adjustment scorecards.

use weather_contracts::JsonValue;

use crate::forecast_scorecard_family::validate_family;
use crate::forecast_scorecard_fields::{
    ScorecardClock, ScorecardContractError, member, require_equal, require_instant, require_keys,
    require_match, require_object, require_sha256,
};
use crate::forecast_scorecard_v2::validate_scorecard_v2;
use crate::maintenance_canonical_json::text;

pub const FORECAST_SCORECARD_V1: &str = "forecast-adjustment-scorecard/v1";
pub const FORECAST_SCORECARD_V2: &str = "forecast-adjustment-scorecard/v2";
pub const FORECAST_SCORECARD_MAX_BYTES: usize = 512 * 1024;
pub const FORECAST_SCORECARD_V2_MAX_BYTES: usize = 128 * 1024;
pub(crate) const SEVEN_DAYS_MS: i64 = 7 * 86_400_000;
const FAMILIES: &[&str] = &["temperature", "wind", "rain"];

/// Approved codec and retained private-member rule used by the public projection.
pub trait ScorecardContractValues: ScorecardClock {
    /// decode one original json value with the retained codec semantics
    fn decode_scorecard_json(&mut self, bytes: &[u8]) -> Result<JsonValue, ScorecardContractError>;

    /// recognize private confirmation member keys with the existing policy
    fn scorecard_private_member_key(&self, key: &[u16]) -> bool;
}

/// Parse one bounded v1 or v2 document and enforce its complete aggregate contract.
pub fn parse_forecast_adjustment_scorecard(
    bytes: &[u8],
    now_ms: Option<i64>,
    values: &mut impl ScorecardContractValues,
) -> Result<JsonValue, ScorecardContractError> {
    // reject an oversized document before invoking the codec
    if bytes.len() > FORECAST_SCORECARD_MAX_BYTES {
        return Err(ScorecardContractError::new(
            "forecast adjustment scorecard is too large",
        ));
    }
    let document = values.decode_scorecard_json(bytes)?;
    // retain the tighter v2 publication ceiling before semantic admission
    if bytes.len() > FORECAST_SCORECARD_V2_MAX_BYTES
        && is_wire(member(&document, "contractVersion"), FORECAST_SCORECARD_V2)
    {
        return Err(ScorecardContractError::new(
            "forecast adjustment scorecard v2 is too large",
        ));
    }
    validate_forecast_adjustment_scorecard(&document, now_ms, values)?;
    Ok(document)
}

/// Validate aggregate-only publication without modifying the original document.
pub fn validate_forecast_adjustment_scorecard(
    document: &JsonValue,
    now_ms: Option<i64>,
    values: &impl ScorecardContractValues,
) -> Result<(), ScorecardContractError> {
    require_object(document, "scorecard")?;
    reject_private_members(document, values)?;
    // route the new closed schema without changing legacy version failures
    if is_wire(member(document, "contractVersion"), FORECAST_SCORECARD_V2) {
        return validate_scorecard_v2(document, now_ms, values);
    }
    validate_scorecard_v1(document, now_ms, values)
}

/// Validate the historical review-only scorecard envelope.
fn validate_scorecard_v1(
    document: &JsonValue,
    now_ms: Option<i64>,
    clock: &impl ScorecardClock,
) -> Result<(), ScorecardContractError> {
    require_keys(
        document,
        &[
            "automaticActivationEligible",
            "contractVersion",
            "families",
            "generatedAt",
            "inputs",
            "operatorApprovalRequired",
            "servingChanged",
            "siteKey",
            "validThrough",
        ],
        "scorecard",
    )?;
    require_equal(
        member(document, "contractVersion"),
        &wire(FORECAST_SCORECARD_V1),
        "contractVersion",
    )?;
    require_equal(member(document, "siteKey"), &wire("ballydidean"), "siteKey")?;
    validate_validity(
        member(document, "generatedAt"),
        member(document, "validThrough"),
        now_ms,
        false,
        clock,
    )?;
    require_equal(
        member(document, "servingChanged"),
        &JsonValue::Boolean(false),
        "servingChanged",
    )?;
    require_equal(
        member(document, "automaticActivationEligible"),
        &JsonValue::Boolean(false),
        "automaticActivationEligible",
    )?;
    require_equal(
        member(document, "operatorApprovalRequired"),
        &JsonValue::Boolean(true),
        "operatorApprovalRequired",
    )?;
    validate_inputs(member(document, "inputs"), clock)?;
    validate_families(member(document, "families"), clock)
}

/// Validate the original clocks and optional v2 maximum interval.
pub(crate) fn validate_validity(
    generated: &JsonValue,
    through: &JsonValue,
    now_ms: Option<i64>,
    seven_day_max: bool,
    clock: &impl ScorecardClock,
) -> Result<(), ScorecardContractError> {
    let generated_ms = require_instant(generated, "generatedAt", clock)?;
    let through_ms = require_instant(through, "validThrough", clock)?;
    // keep reversed or overlong windows distinct from a previously valid stale publication
    if through_ms <= generated_ms
        || (seven_day_max && through_ms.saturating_sub(generated_ms) > SEVEN_DAYS_MS)
    {
        return Err(ScorecardContractError::new(
            "forecast adjustment scorecard validity is invalid",
        ));
    }
    // use the captured time only when a reader supplies it
    if now_ms.is_some_and(|now| through_ms <= now) {
        return Err(ScorecardContractError::new(
            "forecast adjustment scorecard is stale",
        ));
    }
    Ok(())
}

/// Bind all report digests, revision, and the published package interval.
fn validate_inputs(
    value: &JsonValue,
    clock: &impl ScorecardClock,
) -> Result<(), ScorecardContractError> {
    require_object(value, "inputs")?;
    require_keys(
        value,
        &[
            "adjustmentEvidenceManifestSha256",
            "adjustmentEvidenceWatermarkSha256",
            "forecastTrainingManifestSha256",
            "localDateFrom",
            "localDateTo",
            "reportSha256s",
            "sourceRevision",
            "targetCutoffAt",
        ],
        "inputs",
    )?;
    validate_input_fields(value, clock)
}

/// Validate the original shared input fields after admitting the version's key set.
pub(crate) fn validate_input_fields(
    value: &JsonValue,
    clock: &impl ScorecardClock,
) -> Result<(), ScorecardContractError> {
    // bind all three original publication digests before reading the dates
    for key in [
        "adjustmentEvidenceManifestSha256",
        "adjustmentEvidenceWatermarkSha256",
        "forecastTrainingManifestSha256",
    ] {
        require_sha256(member(value, key), &format!("inputs.{key}"))?;
    }
    require_match(
        member(value, "localDateFrom"),
        "inputs.localDateFrom",
        is_date_label,
    )?;
    require_match(
        member(value, "localDateTo"),
        "inputs.localDateTo",
        is_date_label,
    )?;
    // compare the admitted fixed-width date labels without calendar coercion
    if text(value, "localDateFrom") > text(value, "localDateTo") {
        return Err(ScorecardContractError::new(
            "scorecard input date interval is invalid",
        ));
    }
    require_match(
        member(value, "sourceRevision"),
        "inputs.sourceRevision",
        is_source_revision,
    )?;
    require_instant(
        member(value, "targetCutoffAt"),
        "inputs.targetCutoffAt",
        clock,
    )?;
    let reports = member(value, "reportSha256s");
    require_object(reports, "inputs.reportSha256s")?;
    require_keys(reports, FAMILIES, "inputs.reportSha256s")?;
    // bind one exact report digest for each existing model family
    for family in FAMILIES {
        require_sha256(
            member(reports, family),
            &format!("inputs.reportSha256s.{family}"),
        )?;
    }
    Ok(())
}

/// Validate the same complete family cards in both schemas.
pub(crate) fn validate_families(
    families: &JsonValue,
    clock: &impl ScorecardClock,
) -> Result<(), ScorecardContractError> {
    require_object(families, "families")?;
    require_keys(families, FAMILIES, "families")?;
    // retain the exact original family validation order
    for family in FAMILIES {
        validate_family(member(families, family), family, clock)?;
    }
    Ok(())
}

/// Walk the decoded field tree in predecessor order using the retained policy.
fn reject_private_members(
    root: &JsonValue,
    values: &impl ScorecardContractValues,
) -> Result<(), ScorecardContractError> {
    let mut pending = vec![(root, "scorecard".to_owned(), None)];
    // keep traversal iterative while preserving the first encountered diagnostic
    while let Some((value, path, key)) = pending.pop() {
        // evaluate each field before traversing its descendants
        if key.is_some_and(|key| values.scorecard_private_member_key(key)) {
            return Err(ScorecardContractError::new(format!("{path} is private")));
        }
        // push siblings in reverse so that the first original member is popped first
        match value {
            JsonValue::Array(entries) => {
                for (index, entry) in entries.iter().enumerate().rev() {
                    pending.push((entry, format!("{path}[{index}]"), None));
                }
            }
            JsonValue::Object(entries) => {
                for (key, entry) in entries.iter().rev() {
                    let name = String::from_utf16_lossy(key);
                    pending.push((entry, format!("{path}.{name}"), Some(key.as_slice())));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Recognize one exact predecessor date label.
fn is_date_label(date: &str) -> bool {
    let date = date.as_bytes();
    date.len() == 10
        && date.iter().enumerate().all(|(index, byte)| {
            // retain exactly the two required ascii separators
            if matches!(index, 4 | 7) {
                *byte == b'-'
            } else {
                byte.is_ascii_digit()
            }
        })
}

/// Recognize a complete lowercase git revision.
fn is_source_revision(revision: &str) -> bool {
    matches!(revision.len(), 40 | 64)
        && revision
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Construct a literal scorecard value without serializing the document.
pub(crate) fn wire(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Compare one original literal directly in utf-16.
pub(crate) fn is_wire(value: &JsonValue, expected: &str) -> bool {
    matches!(value, JsonValue::String(units) if units.iter().copied().eq(expected.encode_utf16()))
}
