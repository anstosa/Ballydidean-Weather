//! Shared admission of the retained monthly rain-maintenance control state.

use weather_contracts::{JsonValue, RainMaintenanceControlStateContractVersionLiteral};

pub const RAIN_CONTROL_STATE_MAXIMUM_BYTES: usize = 32 * 1_024;

const DAY_MILLISECONDS: i64 = 24 * 60 * 60 * 1_000;
const RECIPE_SHA256: &str = "afa04352b82ccb9704a49863c2dd8724d5c129760ca6d9e4760cb1dda72fb34a";
const STATE_KEYS: &[&str] = &[
    "calibrationEndAt",
    "calibrationStartAt",
    "contractVersion",
    "epochWitnessSha256",
    "generatedAt",
    "legacyCalibrationStartAt",
    "legacyRawScale64",
    "modelMonth",
    "ordinalArtifactSha256",
    "recentFallbackReason",
    "recentRawScale64",
    "recentSupported",
    "recipeSha256",
    "sameWindowRawScale64",
    "scheduleContractSha256",
    "sourceMemberRootSha256",
    "sourceReceiptRootSha256",
    "stateSha256",
    "support",
    "trainingMaximumValidAt",
];
const SUPPORT_KEYS: &[&str] = &[
    "calibrationDates",
    "calibrationHours",
    "calibrationRows",
    "calibrationWetDates",
    "calibrationWetHours",
    "effectiveDates64",
    "effectiveWetDates64",
    "legacyCalibrationRows",
    "legacyTrainingRows",
    "legacyTrainingWetRows",
    "trainingDates",
    "trainingHours",
    "trainingRows",
    "trainingWetDates",
    "trainingWetHours",
];
const SUPPORT_COUNTER_KEYS: &[&str] = &[
    "calibrationDates",
    "calibrationHours",
    "calibrationRows",
    "calibrationWetDates",
    "calibrationWetHours",
    "legacyCalibrationRows",
    "legacyTrainingRows",
    "legacyTrainingWetRows",
    "trainingDates",
    "trainingHours",
    "trainingRows",
    "trainingWetDates",
    "trainingWetHours",
];

/// Approved JSON, digest, and UTC primitives for the original Node control-state contract.
pub trait RainMaintenanceControlPrimitives {
    /// Decode one JSON document without normalizing numbers, keys, or strings.
    fn decode_control_json(&self, bytes: &[u8]) -> Option<JsonValue>;

    /// Encode the closed state and support fields in their source-defined order with one LF.
    fn encode_control_ordered_json(&self, doc: &JsonValue) -> Option<Vec<u8>>;

    /// Encode recursively sorted canonical JSON with one LF.
    fn encode_control_sorted_json(&self, doc: &JsonValue) -> Option<Vec<u8>>;

    /// Compare the SHA-256 of exact bytes with one lowercase digest.
    fn control_sha256_matches(&self, bytes: &[u8], expected: &str) -> bool;

    /// Resolve one exact canonical UTC millisecond instant.
    fn control_utc_milliseconds(&self, utc: &str) -> Option<i64>;
}

/// Causal identities and clocks retained after complete control-state validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedRainMaintenanceControlState {
    pub epoch_witness_sha256: String,
    pub model_month: String,
    pub ordinal_artifact_sha256: String,
    pub source_member_root_sha256: String,
    pub source_receipt_root_sha256: String,
    pub generated_at: String,
    pub training_maximum_epoch_milliseconds: i64,
    pub generated_epoch_milliseconds: i64,
    /// Legacy, same-window and recent scale bits in original ordinal order.
    pub volume_scale_bits: [u64; 3],
}

/// Decode and admit one exact self-addressed monthly rain-maintenance control state.
pub fn admit_rain_maintenance_control_state(
    values: &impl RainMaintenanceControlPrimitives,
    bytes: &[u8],
) -> Option<AdmittedRainMaintenanceControlState> {
    // reject empty, non-document, or oversized state before invoking the decoder
    if !(2..=RAIN_CONTROL_STATE_MAXIMUM_BYTES).contains(&bytes.len()) {
        return None;
    }
    let doc = values.decode_control_json(bytes)?;
    ordered_keys(&doc, STATE_KEYS)?;
    let support = field(&doc, "support")?;
    ordered_keys(support, SUPPORT_KEYS)?;
    // require the original ordered compact representation including its one LF
    if values.encode_control_ordered_json(&doc)?.as_slice() != bytes {
        return None;
    }

    // bind the reviewed contract and every immutable graph identity
    if text(&doc, "contractVersion")?
        != RainMaintenanceControlStateContractVersionLiteral::WIRE_VALUE
        || text(&doc, "recipeSha256")? != RECIPE_SHA256
    {
        return None;
    }
    let epoch_witness_sha256 = hash(&doc, "epochWitnessSha256")?;
    let ordinal_artifact_sha256 = hash(&doc, "ordinalArtifactSha256")?;
    hash(&doc, "scheduleContractSha256")?;
    let source_member_root_sha256 = hash(&doc, "sourceMemberRootSha256")?;
    let source_receipt_root_sha256 = hash(&doc, "sourceReceiptRootSha256")?;
    let state_sha256 = hash(&doc, "stateSha256")?;
    let unsigned = without_field(&doc, "stateSha256")?;
    let unsigned_bytes = values.encode_control_sorted_json(&unsigned)?;
    // require the state address to cover every field except itself
    if !values.control_sha256_matches(&unsigned_bytes, &state_sha256) {
        return None;
    }

    let model_month = text(&doc, "modelMonth")?;
    // accept only the original four-digit year and two-digit month spelling
    if !valid_model_month(&model_month) {
        return None;
    }
    let month_start =
        values.control_utc_milliseconds(&format!("{model_month}-01T00:00:00.000Z"))?;
    let calibration_start = instant(values, &doc, "calibrationStartAt")?;
    let calibration_end = instant(values, &doc, "calibrationEndAt")?;
    let legacy_start = instant(values, &doc, "legacyCalibrationStartAt")?;
    let training_maximum = instant(values, &doc, "trainingMaximumValidAt")?;
    let generated = instant(values, &doc, "generatedAt")?;
    let training_embargo = calibration_start.checked_sub(7 * DAY_MILLISECONDS)?;
    // preserve the 90-day, 45-day, monthly embargo, and causal generation bounds
    if calibration_end.checked_sub(calibration_start)? != 90 * DAY_MILLISECONDS
        || calibration_end.checked_sub(legacy_start)? != 45 * DAY_MILLISECONDS
        || month_start.checked_sub(calibration_end)? != 7 * DAY_MILLISECONDS
        || training_maximum >= training_embargo
        || generated < calibration_end
        || generated >= month_start
    {
        return None;
    }

    let legacy_scale = binary64(&doc, "legacyRawScale64")?;
    let same_window_scale = binary64(&doc, "sameWindowRawScale64")?;
    let recent_scale = binary64(&doc, "recentRawScale64")?;
    let recent_supported = boolean(&doc, "recentSupported")?;
    let recent_reason = text(&doc, "recentFallbackReason")?;
    // retain exact scalar bounds and the legacy unsupported fallback equality
    if !(0.5..=2.0).contains(&legacy_scale)
        || !(0.1..=3.0).contains(&same_window_scale)
        || !(0.1..=3.0).contains(&recent_scale)
        || recent_supported != (recent_reason == "recent_calibration")
        || (!recent_supported && recent_scale != same_window_scale)
    {
        return None;
    }
    validate_support(support, recent_supported)?;

    Some(AdmittedRainMaintenanceControlState {
        epoch_witness_sha256,
        model_month,
        ordinal_artifact_sha256,
        source_member_root_sha256,
        source_receipt_root_sha256,
        generated_at: text(&doc, "generatedAt")?,
        training_maximum_epoch_milliseconds: training_maximum,
        generated_epoch_milliseconds: generated,
        volume_scale_bits: [
            legacy_scale.to_bits(),
            same_window_scale.to_bits(),
            recent_scale.to_bits(),
        ],
    })
}

/// Validate exact closed support counters and the retained recent-support decision.
fn validate_support(support: &JsonValue, recent_supported: bool) -> Option<()> {
    // admit every ordinary support member as one nonnegative JS-safe integer
    for key in SUPPORT_COUNTER_KEYS {
        safe_counter(support, key)?;
    }
    let effective_dates = binary64(support, "effectiveDates64")?;
    let effective_wet_dates = binary64(support, "effectiveWetDates64")?;
    let training_dates = safe_counter(support, "trainingDates")?;
    let training_hours = safe_counter(support, "trainingHours")?;
    let training_rows = safe_counter(support, "trainingRows")?;
    let training_wet_dates = safe_counter(support, "trainingWetDates")?;
    let training_wet_hours = safe_counter(support, "trainingWetHours")?;
    let calibration_dates = safe_counter(support, "calibrationDates")?;
    let calibration_hours = safe_counter(support, "calibrationHours")?;
    let calibration_rows = safe_counter(support, "calibrationRows")?;
    let calibration_wet_dates = safe_counter(support, "calibrationWetDates")?;
    let calibration_wet_hours = safe_counter(support, "calibrationWetHours")?;
    let legacy_training_rows = safe_counter(support, "legacyTrainingRows")?;
    let legacy_training_wet_rows = safe_counter(support, "legacyTrainingWetRows")?;
    let legacy_calibration_rows = safe_counter(support, "legacyCalibrationRows")?;
    let common_supported = training_dates >= 180
        && training_hours >= 1_000
        && training_wet_dates >= 20
        && training_wet_hours >= 100
        && calibration_dates >= 60
        && calibration_hours >= 500
        && calibration_wet_dates >= 5
        && calibration_wet_hours >= 20
        && legacy_training_rows >= 1_000
        && legacy_training_wet_rows >= 100
        && legacy_calibration_rows >= 200;
    let effective_supported = effective_dates >= 30.0 && effective_wet_dates >= 3.0;
    // bind the decision to every threshold and reject row counts below hour support
    (recent_supported == (common_supported && effective_supported)
        && training_rows >= training_hours
        && calibration_rows >= calibration_hours)
        .then_some(())
}

/// Borrow one named object member.
fn field<'a>(doc: &'a JsonValue, key: &str) -> Option<&'a JsonValue> {
    let JsonValue::Object(fields) = doc else {
        return None;
    };
    fields
        .iter()
        .find_map(|(name, value)| name.iter().copied().eq(key.encode_utf16()).then_some(value))
}

/// Require the exact source-defined object member order.
fn ordered_keys(doc: &JsonValue, keys: &[&str]) -> Option<()> {
    let JsonValue::Object(fields) = doc else {
        return None;
    };
    (fields.len() == keys.len()
        && fields
            .iter()
            .zip(keys)
            .all(|((actual, _), expected)| actual.iter().copied().eq(expected.encode_utf16())))
    .then_some(())
}

/// Decode one exact UTF-16 string without replacement.
fn text(doc: &JsonValue, key: &str) -> Option<String> {
    let JsonValue::String(units) = field(doc, key)? else {
        return None;
    };
    String::from_utf16(units).ok()
}

/// Read one strict lowercase SHA-256 identity.
fn hash(doc: &JsonValue, key: &str) -> Option<String> {
    let value = text(doc, key)?;
    (value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f')))
    .then_some(value)
}

/// Read one required JSON boolean.
fn boolean(doc: &JsonValue, key: &str) -> Option<bool> {
    let JsonValue::Boolean(value) = field(doc, key)? else {
        return None;
    };
    Some(*value)
}

/// Decode one exact finite lowercase network-order binary64 value.
fn binary64(doc: &JsonValue, key: &str) -> Option<f64> {
    let value = text(doc, key)?;
    // exclude decimal, uppercase, short, and long aliases before reading bits
    if value.len() != 16
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return None;
    }
    let number = f64::from_bits(u64::from_str_radix(&value, 16).ok()?);
    number.is_finite().then_some(number)
}

/// Read one nonnegative exactly represented JavaScript safe integer.
fn safe_counter(doc: &JsonValue, key: &str) -> Option<u64> {
    let JsonValue::Number(value) = field(doc, key)? else {
        return None;
    };
    // preserve Number.isSafeInteger and the original nonnegative bound
    if !value.is_finite()
        || value.fract() != 0.0
        || *value < 0.0
        || *value > 9_007_199_254_740_991.0
    {
        return None;
    }
    Some(*value as u64)
}

/// Resolve one exact canonical millisecond UTC member.
fn instant(
    values: &impl RainMaintenanceControlPrimitives,
    doc: &JsonValue,
    key: &str,
) -> Option<i64> {
    let value = text(doc, key)?;
    // require the source's exact four-digit UTC millisecond spelling
    if !canonical_utc_shape(&value) {
        return None;
    }
    values.control_utc_milliseconds(&value)
}

/// Recognize the exact four-digit year and bounded month spelling.
fn valid_model_month(value: &str) -> bool {
    let bytes = value.as_bytes();
    // admit only YYYY-MM with a decimal month from one through twelve
    bytes.len() == 7
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'-'
        && bytes[5].is_ascii_digit()
        && bytes[6].is_ascii_digit()
        && matches!(
            &bytes[5..],
            b"01"
                | b"02"
                | b"03"
                | b"04"
                | b"05"
                | b"06"
                | b"07"
                | b"08"
                | b"09"
                | b"10"
                | b"11"
                | b"12"
        )
}

/// Recognize the exact canonical UTC-millisecond lexical shape.
fn canonical_utc_shape(value: &str) -> bool {
    let bytes = value.as_bytes();
    // match YYYY-MM-DDTHH:mm:ss.sssZ before the injected calendar validates it
    bytes.len() == 24
        && [0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18, 20, 21, 22]
            .iter()
            .all(|index| bytes[*index].is_ascii_digit())
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b'T'
        && bytes[13] == b':'
        && bytes[16] == b':'
        && bytes[19] == b'.'
        && bytes[23] == b'Z'
}

/// Clone one object while omitting exactly its self-address field.
fn without_field(doc: &JsonValue, omitted: &str) -> Option<JsonValue> {
    let JsonValue::Object(fields) = doc else {
        return None;
    };
    Some(JsonValue::Object(
        fields
            .iter()
            .filter(|(key, _)| !key.iter().copied().eq(omitted.encode_utf16()))
            .cloned()
            .collect(),
    ))
}
