//! Exact producer admission for the rain feature and control projections.

use std::cmp::Ordering;

use weather_contracts::{AdjustmentRevisionProjectionKind, JsonValue};
use weather_domain::{RAIN_COLLECTION_STATIONS, RainCollectionStation};

use crate::archive_wire::{
    ArchiveWireCodec, ArchiveWirePrimitives, array, exact_keys, field, hash, member, text,
};
use crate::private_revision_archive::{
    AdmittedPrivateRevisionProjection, PrivateRevisionProjectionDependency,
    PrivateRevisionProjectionShape,
};
use crate::producer_projection::{
    binary64, family_limit, instant_field, integer, nullable_binary64, ordered,
};
use crate::revision_archive_canonical::{RevisionArchiveCanonicalCodec, lower_hex};

const TOP_KEYS: &[&str] = &[
    "contractVersion",
    "family",
    "logicalKey",
    "logicalReceivedAt",
    "projectionKind",
    "rows",
    "source",
    "storedContentSha256",
];
const CONTROL_TOP_KEYS: &[&str] = &[
    "contractVersion",
    "family",
    "logicalKey",
    "logicalReceivedAt",
    "ordinalArtifactSha256",
    "persistenceTarget",
    "projectionKind",
    "rows",
    "source",
    "stateSha256",
    "stateStageReceiptSha256",
    "storedContentSha256",
];
const LOGICAL_KEY_KEYS: &[&str] = &["inputSha256", "modelSha256", "runInitializedAt"];
const SOURCE_KEYS: &[&str] = &[
    "adapterVersion",
    "contractEpoch",
    "dataset",
    "providerKey",
    "sourceConfigFingerprint",
    "sourceId",
    "sourceKey",
    "sourceKind",
    "upstreamModel",
];
const FEATURE_ROW_KEYS: &[&str] = &[
    "features64",
    "modelLeadHours",
    "rawPrecipitationMm64",
    "rawTargetHourTemperatureC64",
    "validAt",
];
const CONTROL_ROW_KEYS: &[&str] = &[
    "features64",
    "incumbentArtifactIdentitySha256",
    "incumbentPrediction64",
    "incumbentProbability",
    "incumbentReceiptMemberSha256",
    "modelLeadHours",
    "nativeSourceProbability",
    "persistencePrediction64",
    "persistenceReason",
    "persistenceTargetMemberSha256",
    "rawPrecipitationMm64",
    "rawTargetHourTemperatureC64",
    "recentVolumeScalePrediction64",
    "sameWindowVolumeScalePrediction64",
    "sourceRowSha256",
    "unchangedOrdinalPrediction64",
    "validAt",
    "volumeScalePrediction64",
];
const PROBABILITY_KEYS: &[&str] = &["atLeast0_1", "atLeast1_0", "atLeast2_5"];
const PERSISTENCE_KEYS: &[&str] = &[
    "contractVersion",
    "decisionAt",
    "prediction64",
    "reason",
    "rowCount",
    "rows",
    "targetMemberSha256",
    "validAt",
];
const PERSISTENCE_ROW_KEYS: &[&str] = &[
    "captureMembers",
    "precipitationMm64",
    "receivedAt",
    "stationId",
];
const CAPTURE_KEYS: &[&str] = &["bodySha256", "claimId", "completedAt"];
const HOUR_MILLISECONDS: i64 = 3_600_000;
const RAIN_ROW_COUNT: usize = 23;
const FEATURE_COUNT: usize = 107;
const SOURCE_CONFIG_FINGERPRINT: &str =
    "96e4365f74a0fb8694944172b73a65c29d89a68ddf86b1b0a50db38bb39d52aa";
// exact binary64 weights of the original compiled catalog in station order
const STATION_WEIGHTS: [f64; 12] = [
    f64::from_bits(0x3feac6b3c3e9617f),
    f64::from_bits(0x3fea359b380542db),
    f64::from_bits(0x3fe8e968b3225631),
    f64::from_bits(0x3fe8cdec5703a511),
    f64::from_bits(0x3fe57d6bda81c16d),
    f64::from_bits(0x3fe5757fd13c391e),
    f64::from_bits(0x3fe307ef06f37997),
    f64::from_bits(0x3fde1225a651824a),
    f64::from_bits(0x3fcafe7688724d40),
    f64::from_bits(0x3fca49b09d235947),
    f64::from_bits(0x3fc812ce46bfbc06),
    f64::from_bits(0x3fc2b2faab23c3d3),
];

/// One validated causal target needed while checking all control rows.
struct PersistenceTarget {
    ordered: JsonValue,
    prediction: Option<f64>,
    target_member_sha256: String,
}

/// One weighted physical gauge value used by the deterministic median.
#[derive(Clone, Copy)]
struct WeightedValue {
    id: u64,
    value: f64,
    weight: f64,
}

/// Admit one exact rain producer projection without retaining its large value tree.
pub(crate) fn admit_rain_projection<P: ArchiveWirePrimitives>(
    codec: &ArchiveWireCodec<P>,
    value: &JsonValue,
    bytes: &[u8],
) -> Option<AdmittedPrivateRevisionProjection> {
    // enforce the same family cap when this lane is called directly
    if !(2..=family_limit("rain")?).contains(&bytes.len()) {
        return None;
    }
    let contract = text(value, "contractVersion")?;
    let (canonical, dependency) = match contract.as_str() {
        "adjustment-rain-gate-feature-projection/v2" => (
            validate_feature_projection(codec, value)?,
            PrivateRevisionProjectionDependency::None,
        ),
        "adjustment-rain-gate-control-projection/v3" => validate_control_projection(codec, value)?,
        _ => return None,
    };
    // reject every order, whitespace, number, and duplicate-field alias
    if codec.encode_ordered(&canonical)?.as_slice() != bytes {
        return None;
    }
    Some(AdmittedPrivateRevisionProjection {
        projection_kind: AdjustmentRevisionProjectionKind::RainGateInput,
        shape: PrivateRevisionProjectionShape::Single,
        dependency,
    })
}

/// Validate and order the complete pre-fit feature projection.
fn validate_feature_projection<P: ArchiveWirePrimitives>(
    codec: &ArchiveWireCodec<P>,
    value: &JsonValue,
) -> Option<JsonValue> {
    // close the additive feature grammar on the original rain receipt class
    if !exact_keys(value, TOP_KEYS)
        || text(value, "contractVersion")? != "adjustment-rain-gate-feature-projection/v2"
        || text(value, "family")? != "rain"
        || text(value, "projectionKind")? != "rain_gate_input"
    {
        return None;
    }
    instant_field(codec, value, "logicalReceivedAt")?;
    hash(value, "storedContentSha256")?;
    let logical_key = field(value, "logicalKey")?;
    let initialized_at = validate_logical_key(codec, logical_key)?;
    let source = field(value, "source")?;
    validate_source(source)?;
    let rows = validate_feature_rows(codec, array(value, "rows")?, initialized_at)?;
    Some(JsonValue::Object(vec![
        member("contractVersion", field(value, "contractVersion")?.clone()),
        member("family", field(value, "family")?.clone()),
        member("logicalKey", ordered(logical_key, LOGICAL_KEY_KEYS)?),
        member(
            "logicalReceivedAt",
            field(value, "logicalReceivedAt")?.clone(),
        ),
        member("projectionKind", field(value, "projectionKind")?.clone()),
        member("rows", JsonValue::Array(rows)),
        member("source", ordered(source, SOURCE_KEYS)?),
        member(
            "storedContentSha256",
            field(value, "storedContentSha256")?.clone(),
        ),
    ]))
}

/// Validate and order all twenty-three feature rows.
fn validate_feature_rows<P: ArchiveWirePrimitives>(
    codec: &ArchiveWireCodec<P>,
    rows: &[JsonValue],
    initialized_at: i64,
) -> Option<Vec<JsonValue>> {
    // preserve the complete native training geometry
    if rows.len() != RAIN_ROW_COUNT {
        return None;
    }
    let mut result = Vec::with_capacity(rows.len());
    // validate every exact feature vector and target clock
    for (index, row) in rows.iter().enumerate() {
        // reject additions or omissions before rebuilding ordered bytes
        if !exact_keys(row, FEATURE_ROW_KEYS) {
            return None;
        }
        validate_feature_fields(codec, row, index, initialized_at)?;
        result.push(ordered(row, FEATURE_ROW_KEYS)?);
    }
    Some(result)
}

/// Validate the shared feature-bearing portion of one row.
fn validate_feature_fields<P: ArchiveWirePrimitives>(
    codec: &ArchiveWireCodec<P>,
    row: &JsonValue,
    index: usize,
    initialized_at: i64,
) -> Option<()> {
    let expected_lead = i64::try_from(index).ok()?.checked_add(9)?;
    let valid_at = instant_field(codec, row, "validAt")?;
    // bind the one-based serving lead to its exact model-run clock
    if integer(field(row, "modelLeadHours")?, 1, 168)? != expected_lead
        || valid_at != initialized_at.checked_add(expected_lead.checked_mul(HOUR_MILLISECONDS)?)?
    {
        return None;
    }
    let raw = binary64(field(row, "rawPrecipitationMm64")?)?;
    binary64(field(row, "rawTargetHourTemperatureC64")?)?;
    let features = array(row, "features64")?;
    // retain only the exact 107-value physical feature geometry
    if raw < 0.0 || features.len() != FEATURE_COUNT {
        return None;
    }
    // preserve explicit null predictors and finite binary64 values
    for feature in features {
        nullable_binary64(feature)?;
    }
    Some(())
}

/// Validate and order the complete pre-target control projection.
fn validate_control_projection<P: ArchiveWirePrimitives>(
    codec: &ArchiveWireCodec<P>,
    value: &JsonValue,
) -> Option<(JsonValue, PrivateRevisionProjectionDependency)> {
    // close the control grammar on the original rain receipt class
    if !exact_keys(value, CONTROL_TOP_KEYS)
        || text(value, "contractVersion")? != "adjustment-rain-gate-control-projection/v3"
        || text(value, "family")? != "rain"
        || text(value, "projectionKind")? != "rain_gate_input"
    {
        return None;
    }
    instant_field(codec, value, "logicalReceivedAt")?;
    let ordinal_artifact_sha256 = hash(value, "ordinalArtifactSha256")?;
    let state_sha256 = hash(value, "stateSha256")?;
    let state_stage_receipt_sha256 = hash(value, "stateStageReceiptSha256")?;
    hash(value, "storedContentSha256")?;
    let logical_key = field(value, "logicalKey")?;
    let initialized_at = validate_logical_key(codec, logical_key)?;
    let ordered_key = ordered(logical_key, LOGICAL_KEY_KEYS)?;
    let source = field(value, "source")?;
    validate_source(source)?;
    let persistence = validate_persistence_target(codec, field(value, "persistenceTarget")?)?;
    let rows = validate_control_rows(
        codec,
        array(value, "rows")?,
        initialized_at,
        &ordered_key,
        &persistence,
    )?;
    let canonical = JsonValue::Object(vec![
        member("contractVersion", field(value, "contractVersion")?.clone()),
        member("family", field(value, "family")?.clone()),
        member("logicalKey", ordered_key),
        member(
            "logicalReceivedAt",
            field(value, "logicalReceivedAt")?.clone(),
        ),
        member(
            "ordinalArtifactSha256",
            field(value, "ordinalArtifactSha256")?.clone(),
        ),
        member("persistenceTarget", persistence.ordered),
        member("projectionKind", field(value, "projectionKind")?.clone()),
        member("rows", JsonValue::Array(rows)),
        member("source", ordered(source, SOURCE_KEYS)?),
        member("stateSha256", field(value, "stateSha256")?.clone()),
        member(
            "stateStageReceiptSha256",
            field(value, "stateStageReceiptSha256")?.clone(),
        ),
        member(
            "storedContentSha256",
            field(value, "storedContentSha256")?.clone(),
        ),
    ]);
    Some((
        canonical,
        PrivateRevisionProjectionDependency::RainControlState {
            ordinal_artifact_sha256,
            state_sha256,
            state_stage_receipt_sha256,
        },
    ))
}

/// Validate and order all control rows while retaining one incumbent authority.
fn validate_control_rows<P: ArchiveWirePrimitives>(
    codec: &ArchiveWireCodec<P>,
    rows: &[JsonValue],
    initialized_at: i64,
    logical_key: &JsonValue,
    persistence: &PersistenceTarget,
) -> Option<Vec<JsonValue>> {
    // preserve the complete native training and control geometry
    if rows.len() != RAIN_ROW_COUNT {
        return None;
    }
    let mut artifact_identity: Option<Option<String>> = None;
    let mut receipt_identity: Option<String> = None;
    let mut result = Vec::with_capacity(rows.len());
    // bind every control to the same feature row and incumbent authority
    for (index, row) in rows.iter().enumerate() {
        if !exact_keys(row, CONTROL_ROW_KEYS) {
            return None;
        }
        validate_feature_fields(codec, row, index, initialized_at)?;
        let incumbent_probability = validate_probability(field(row, "incumbentProbability")?)?;
        let native_probability = validate_probability(field(row, "nativeSourceProbability")?)?;
        let receipt = hash(row, "incumbentReceiptMemberSha256")?;
        let source_row_sha256 = hash(row, "sourceRowSha256")?;
        let artifact = optional_hash(field(row, "incumbentArtifactIdentitySha256")?)?;
        let expected_source_row_sha256 = feature_row_sha256(codec, logical_key, row)?;
        // reject controls detached from the exact source feature row
        if source_row_sha256 != expected_source_row_sha256 {
            return None;
        }
        validate_control_values(row, &native_probability, persistence)?;
        // fix the first row's incumbent authority for the complete body
        if index == 0 {
            artifact_identity = Some(artifact.clone());
            receipt_identity = Some(receipt.clone());
        }
        // prevent one projection from mixing serving authorities
        if artifact_identity.as_ref()? != &artifact || receipt_identity.as_ref()? != &receipt {
            return None;
        }
        result.push(JsonValue::Object(vec![
            member("features64", field(row, "features64")?.clone()),
            member(
                "incumbentArtifactIdentitySha256",
                field(row, "incumbentArtifactIdentitySha256")?.clone(),
            ),
            member(
                "incumbentPrediction64",
                field(row, "incumbentPrediction64")?.clone(),
            ),
            member("incumbentProbability", incumbent_probability.0),
            member(
                "incumbentReceiptMemberSha256",
                field(row, "incumbentReceiptMemberSha256")?.clone(),
            ),
            member("modelLeadHours", field(row, "modelLeadHours")?.clone()),
            member("nativeSourceProbability", native_probability.0),
            member(
                "persistencePrediction64",
                field(row, "persistencePrediction64")?.clone(),
            ),
            member(
                "persistenceReason",
                field(row, "persistenceReason")?.clone(),
            ),
            member(
                "persistenceTargetMemberSha256",
                field(row, "persistenceTargetMemberSha256")?.clone(),
            ),
            member(
                "rawPrecipitationMm64",
                field(row, "rawPrecipitationMm64")?.clone(),
            ),
            member(
                "rawTargetHourTemperatureC64",
                field(row, "rawTargetHourTemperatureC64")?.clone(),
            ),
            member(
                "recentVolumeScalePrediction64",
                field(row, "recentVolumeScalePrediction64")?.clone(),
            ),
            member(
                "sameWindowVolumeScalePrediction64",
                field(row, "sameWindowVolumeScalePrediction64")?.clone(),
            ),
            member("sourceRowSha256", field(row, "sourceRowSha256")?.clone()),
            member(
                "unchangedOrdinalPrediction64",
                field(row, "unchangedOrdinalPrediction64")?.clone(),
            ),
            member("validAt", field(row, "validAt")?.clone()),
            member(
                "volumeScalePrediction64",
                field(row, "volumeScalePrediction64")?.clone(),
            ),
        ]));
    }
    Some(result)
}

/// Validate one nested probability vector and retain its decoded values.
fn validate_probability(value: &JsonValue) -> Option<(JsonValue, [f64; 3])> {
    // require exactly the three ordered event heads
    if !exact_keys(value, PROBABILITY_KEYS) {
        return None;
    }
    let decoded = [
        binary64(field(value, PROBABILITY_KEYS[0])?)?,
        binary64(field(value, PROBABILITY_KEYS[1])?)?,
        binary64(field(value, PROBABILITY_KEYS[2])?)?,
    ];
    // retain only bounded nested exceedance probabilities
    if decoded
        .iter()
        .any(|probability| !(0.0..=1.0).contains(probability))
        || decoded[0] < decoded[1]
        || decoded[1] < decoded[2]
    {
        return None;
    }
    Some((ordered(value, PROBABILITY_KEYS)?, decoded))
}

/// Validate physical controls and their exact causal persistence binding.
fn validate_control_values(
    row: &JsonValue,
    native_probability: &(JsonValue, [f64; 3]),
    persistence: &PersistenceTarget,
) -> Option<()> {
    let raw = binary64(field(row, "rawPrecipitationMm64")?)?;
    let prediction = binary64(field(row, "persistencePrediction64")?)?;
    let nonnegative = [
        "incumbentPrediction64",
        "recentVolumeScalePrediction64",
        "sameWindowVolumeScalePrediction64",
        "unchangedOrdinalPrediction64",
        "volumeScalePrediction64",
    ];
    // require every amount control and incumbent to remain physical
    if raw < 0.0 || prediction < 0.0 {
        return None;
    }
    // force each named control to contain a finite binary64
    for name in nonnegative {
        // reject every nonphysical amount without clipping it
        if binary64(field(row, name)?)? < 0.0 {
            return None;
        }
    }
    let expected_native = [
        f64::from(u8::from(raw >= 0.1)),
        f64::from(u8::from(raw >= 1.0)),
        f64::from(u8::from(raw >= 2.5)),
    ];
    // retain the deterministic raw-source event baseline exactly
    if native_probability.1 != expected_native {
        return None;
    }
    // bind an unavailable target to the raw fallback representation
    if persistence.prediction.is_none() {
        return (text(row, "persistenceReason")? == "raw_fallback_unavailable"
            && matches!(
                field(row, "persistenceTargetMemberSha256")?,
                JsonValue::Null
            )
            && prediction == raw)
            .then_some(());
    }
    // bind an available target to its exact member and numerical prediction
    (text(row, "persistenceReason")? == "causal_target"
        && hash(row, "persistenceTargetMemberSha256")? == persistence.target_member_sha256
        && prediction == persistence.prediction?)
        .then_some(())
}

/// Recompute the feature-row identity from the original ordered preimage.
fn feature_row_sha256<P: ArchiveWirePrimitives>(
    codec: &ArchiveWireCodec<P>,
    logical_key: &JsonValue,
    row: &JsonValue,
) -> Option<String> {
    let feature_row = ordered(row, FEATURE_ROW_KEYS)?;
    let mut preimage = b"adjustment-rain-gate-feature-row/v2\n".to_vec();
    preimage.extend(codec.encode_ordered(logical_key)?);
    preimage.extend(codec.encode_ordered(&feature_row)?);
    Some(lower_hex(codec.sha256(&preimage)))
}

/// Validate and canonically order one causal prior-hour persistence target.
fn validate_persistence_target<P: ArchiveWirePrimitives>(
    codec: &ArchiveWireCodec<P>,
    value: &JsonValue,
) -> Option<PersistenceTarget> {
    // close and self-bind the complete persistence target member
    if !exact_keys(value, PERSISTENCE_KEYS)
        || text(value, "contractVersion")? != "rain-maintenance-persistence-target/v1"
    {
        return None;
    }
    let decision_at = instant_field(codec, value, "decisionAt")?;
    let valid_at = instant_field(codec, value, "validAt")?;
    let target_member_sha256 = hash(value, "targetMemberSha256")?;
    let rows = array(value, "rows")?;
    // retain exactly one prior hour and all fixed catalog rows
    if decision_at.checked_sub(valid_at)? != HOUR_MILLISECONDS
        || integer(field(value, "rowCount")?, 12, 12)? != 12
        || rows.len() != RAIN_COLLECTION_STATIONS.len()
        || canonical_object_sha256(codec, value, "targetMemberSha256")? != target_member_sha256
    {
        return None;
    }
    let mut available = Vec::with_capacity(rows.len());
    let mut nearest_available = false;
    let mut ordered_rows = Vec::with_capacity(rows.len());
    // preserve every fixed gauge including explicitly unavailable rows
    for (index, row) in rows.iter().enumerate() {
        let station = RAIN_COLLECTION_STATIONS.get(index)?;
        let (ordered_row, precipitation) =
            validate_persistence_row(codec, row, station, decision_at)?;
        // remember whether one of the nearest three gauges is available
        if index < 3 && precipitation.is_some() {
            nearest_available = true;
        }
        // retain each available physical value for the network median
        if let Some(value) = precipitation {
            available.push(WeightedValue {
                id: station.location_id,
                value,
                weight: *STATION_WEIGHTS.get(index)?,
            });
        }
        ordered_rows.push(ordered_row);
    }
    let supported = available.len() >= 3 && nearest_available;
    let prediction = match (supported, field(value, "prediction64")?) {
        (true, encoded) => Some(binary64(encoded)?),
        (false, JsonValue::Null) => None,
        _ => return None,
    };
    // supported members carry the exact deterministic weighted median
    if supported {
        if text(value, "reason")? != "causal_target"
            || prediction? != weighted_median(&mut available)?
        {
            return None;
        }
    // unsupported members retain no invented target value
    } else if text(value, "reason")? != "raw_fallback_unavailable" {
        return None;
    }
    let ordered = JsonValue::Object(vec![
        member("contractVersion", field(value, "contractVersion")?.clone()),
        member("decisionAt", field(value, "decisionAt")?.clone()),
        member("prediction64", field(value, "prediction64")?.clone()),
        member("reason", field(value, "reason")?.clone()),
        member("rowCount", field(value, "rowCount")?.clone()),
        member("rows", JsonValue::Array(ordered_rows)),
        member(
            "targetMemberSha256",
            field(value, "targetMemberSha256")?.clone(),
        ),
        member("validAt", field(value, "validAt")?.clone()),
    ]);
    Some(PersistenceTarget {
        ordered,
        prediction,
        target_member_sha256,
    })
}

/// Validate one fixed-gauge persistence row and all capture members.
fn validate_persistence_row<P: ArchiveWirePrimitives>(
    codec: &ArchiveWireCodec<P>,
    row: &JsonValue,
    station: &RainCollectionStation,
    decision_at: i64,
) -> Option<(JsonValue, Option<f64>)> {
    // require the exact row fields and catalog station identity
    if !exact_keys(row, PERSISTENCE_ROW_KEYS)
        || integer(field(row, "stationId")?, 0, 9_007_199_254_740_991)?
            != i64::try_from(station.location_id).ok()?
    {
        return None;
    }
    let captures = array(row, "captureMembers")?;
    let mut previous_order: Option<Vec<u16>> = None;
    let mut completed_at_values = Vec::with_capacity(captures.len());
    let mut ordered_captures = Vec::with_capacity(captures.len());
    // bind every overlapping provider response used for this gauge hour
    for capture in captures {
        if !exact_keys(capture, CAPTURE_KEYS) {
            return None;
        }
        hash(capture, "bodySha256")?;
        let completed_at = text(capture, "completedAt")?;
        let completed_at_milliseconds = instant_field(codec, capture, "completedAt")?;
        let JsonValue::String(claim_id) = field(capture, "claimId")? else {
            return None;
        };
        // preserve the original JavaScript UTF-16 length without restricting claim contents
        if !(1..=128).contains(&claim_id.len()) || completed_at_milliseconds > decision_at {
            return None;
        }
        let mut capture_order: Vec<u16> = completed_at.encode_utf16().collect();
        capture_order.push(u16::from(b'\n'));
        capture_order.extend(claim_id);
        // require the exact completed-at then claim-id ordering
        if previous_order
            .as_ref()
            .is_some_and(|previous| capture_order.as_slice() <= previous.as_slice())
        {
            return None;
        }
        previous_order = Some(capture_order);
        completed_at_values.push(completed_at);
        ordered_captures.push(ordered(capture, CAPTURE_KEYS)?);
    }
    let received_at = match field(row, "receivedAt")? {
        JsonValue::Null => None,
        _ => {
            let clock = text(row, "receivedAt")?;
            let milliseconds = instant_field(codec, row, "receivedAt")?;
            // bind the selected receipt to one causal capture completion
            if milliseconds > decision_at || !completed_at_values.contains(&clock) {
                return None;
            }
            Some(clock)
        }
    };
    let precipitation = match field(row, "precipitationMm64")? {
        JsonValue::Null => None,
        encoded => {
            let decoded = binary64(encoded)?;
            // physical target values require one selected causal capture
            if decoded < 0.0 || captures.is_empty() || received_at.is_none() {
                return None;
            }
            Some(decoded)
        }
    };
    Some((
        JsonValue::Object(vec![
            member("captureMembers", JsonValue::Array(ordered_captures)),
            member(
                "precipitationMm64",
                field(row, "precipitationMm64")?.clone(),
            ),
            member("receivedAt", field(row, "receivedAt")?.clone()),
            member("stationId", field(row, "stationId")?.clone()),
        ]),
        precipitation,
    ))
}

/// Select the first value reaching the deterministic half-mass boundary.
fn weighted_median(values: &mut [WeightedValue]) -> Option<f64> {
    values.sort_by(|left, right| {
        left.value
            .partial_cmp(&right.value)
            .unwrap_or(Ordering::Equal)
            .then(left.id.cmp(&right.id))
    });
    let half = values.iter().map(|item| item.weight).sum::<f64>() / 2.0;
    let mut cumulative = 0.0;
    // return the lower deterministic value at an exact half-mass tie
    for item in values {
        cumulative += item.weight;
        // preserve the source's inclusive crossing rule
        if cumulative >= half {
            return Some(item.value);
        }
    }
    None
}

/// Validate the fixed logical rain input key and return its initialized clock.
fn validate_logical_key<P: ArchiveWirePrimitives>(
    codec: &ArchiveWireCodec<P>,
    value: &JsonValue,
) -> Option<i64> {
    // bind every rain row to the exact input and model identity
    if !exact_keys(value, LOGICAL_KEY_KEYS) {
        return None;
    }
    hash(value, "inputSha256")?;
    hash(value, "modelSha256")?;
    instant_field(codec, value, "runInitializedAt")
}

/// Validate the fixed rain provider lineage while retaining its actual claim id.
fn validate_source(value: &JsonValue) -> Option<()> {
    // close all fixed adapter fields without normalizing the source identifier
    if !exact_keys(value, SOURCE_KEYS)
        || text(value, "adapterVersion")? != "rain-hurdle-wind-features/v1"
        || text(value, "contractEpoch")? != "rain-prospective-capture/v1"
        || text(value, "dataset")? != "ecmwf_ifs"
        || text(value, "providerKey")? != "open-meteo-single-runs"
        || text(value, "sourceConfigFingerprint")? != SOURCE_CONFIG_FINGERPRINT
        || text(value, "sourceKey")? != "rain-prospective-forecast"
        || text(value, "sourceKind")? != "forecast"
        || text(value, "upstreamModel")? != "ecmwf_ifs"
    {
        return None;
    }
    let JsonValue::String(source_id) = field(value, "sourceId")? else {
        return None;
    };
    (1..=128).contains(&source_id.len()).then_some(())
}

/// Admit a lowercase hash or the contract's explicit null.
fn optional_hash(value: &JsonValue) -> Option<Option<String>> {
    match value {
        JsonValue::Null => Some(None),
        _ => {
            let wrapper = JsonValue::Object(vec![member("value", value.clone())]);
            hash(&wrapper, "value").map(Some)
        }
    }
}

/// Hash sorted canonical object bytes while omitting only its own hash field.
fn canonical_object_sha256<P: ArchiveWirePrimitives>(
    codec: &ArchiveWireCodec<P>,
    value: &JsonValue,
    own_hash_field: &str,
) -> Option<String> {
    let JsonValue::Object(fields) = value else {
        return None;
    };
    let own_hash_units: Vec<u16> = own_hash_field.encode_utf16().collect();
    let material = JsonValue::Object(
        fields
            .iter()
            .filter(
                // omit only the member's self-referential digest
                |(name, _)| name != &own_hash_units,
            )
            .cloned()
            .collect(),
    );
    Some(lower_hex(codec.sha256(&codec.encode(&material)?)))
}
