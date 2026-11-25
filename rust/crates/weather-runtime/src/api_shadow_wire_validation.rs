//! Original shadow JSON schemas and family limits after generic ordered JSON decoding.

use weather_contracts::JsonValue;

use crate::ApiMaintenanceReceiptCanonicalValues;
use crate::api_maintenance_scalar_admission::capture_cycle;
use crate::maintenance_canonical_json::{field, text};

pub(crate) const HOUR_MS: i64 = 3_600_000;
pub(crate) const SHADOW_BODY_MAX_BYTES: usize = 65_536;
pub(crate) const SHADOW_SOURCE_MAX_BYTES: usize = 768 * 1_024;

const BODY_KEYS: &[&str] = &[
    "contractVersion",
    "family",
    "registrationSha256",
    "candidateSha256",
    "sourceSha256",
    "dueKey",
    "issuedAt",
    "sourceReceiptSha256",
    "inputSha256",
    "rowCount",
    "rows",
];
const TEMPERATURE_ROW: &[&str] = &[
    "validAt",
    "leadHours",
    "sourceRowSha256",
    "candidateTemperatureC64",
    "wouldApply",
    "fallbackCode",
];
const WIND_ROW: &[&str] = &[
    "validAt",
    "leadHours",
    "sourceRowSha256",
    "candidateSpeedMps64",
    "candidateGustMps64",
    "speedWouldApply",
    "gustWouldApply",
];
const RAIN_ROW: &[&str] = &[
    "validAt",
    "leadHours",
    "sourceRowSha256",
    "occurrenceProbability64",
    "positiveAmountMm64",
    "candidatePrecipitationMm64",
    "atLeast1_0Probability64",
    "atLeast2_5Probability64",
    "wouldApply",
    "fallbackCode",
];

/// One original shadow document family with its fixed row and byte limits.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShadowFamily {
    Temperature,
    Wind,
    Rain,
}

impl ShadowFamily {
    /// Read the exact original wire family.
    pub(crate) fn read(doc: &JsonValue) -> Option<Self> {
        match text(doc, "family")?.as_str() {
            "temperature" => Some(Self::Temperature),
            "wind" => Some(Self::Wind),
            "rain" => Some(Self::Rain),
            _ => None,
        }
    }

    /// Return the complete family lead geometry.
    pub(crate) const fn row_count(self) -> usize {
        match self {
            Self::Temperature => 12,
            Self::Wind => 168,
            Self::Rain => 23,
        }
    }

    /// Return the original maximum source size including LF.
    pub(crate) const fn source_byte_cap(self) -> usize {
        match self {
            Self::Temperature => 64 * 1_024,
            Self::Wind => SHADOW_SOURCE_MAX_BYTES,
            Self::Rain => 128 * 1_024,
        }
    }

    /// Return the original maximum compact prediction size including LF.
    const fn body_byte_cap(self) -> usize {
        match self {
            Self::Temperature => 8_192,
            Self::Wind => SHADOW_BODY_MAX_BYTES,
            Self::Rain => 12_288,
        }
    }

    /// Return the immutable prediction row order.
    const fn body_row_keys(self) -> &'static [&'static str] {
        match self {
            Self::Temperature => TEMPERATURE_ROW,
            Self::Wind => WIND_ROW,
            Self::Rain => RAIN_ROW,
        }
    }
}

/// Validate the original complete shadow body independent of its source binding.
pub(crate) fn validate_shadow_body(
    values: &impl ApiMaintenanceReceiptCanonicalValues,
    doc: &JsonValue,
    byte_len: usize,
) -> Option<()> {
    ordered_keys(doc, BODY_KEYS)?;
    let family = ShadowFamily::read(doc)?;
    // reject alternate versions and family byte sizes before walking rows
    if text(doc, "contractVersion")? != "adjustment-shadow-prediction-values/v3"
        || byte_len > family.body_byte_cap()
    {
        return None;
    }
    // admit the canonical header identities
    for key in [
        "registrationSha256",
        "candidateSha256",
        "sourceSha256",
        "sourceReceiptSha256",
        "inputSha256",
    ] {
        hash(doc, key)?;
    }
    let issued = instant(values, doc, "issuedAt")?;
    due_key(values, doc)?;
    let rows = rows(doc, "rows")?;
    // require every expected lead and its declared count
    if rows.len() != family.row_count() || unsigned(doc, "rowCount")? != rows.len() as u64 {
        return None;
    }
    let mut previous: Option<i64> = None;
    // walk each exact original row in prospective hourly order
    for (offset, row) in rows.iter().enumerate() {
        ordered_keys(row, family.body_row_keys())?;
        let valid_at = instant(values, row, "validAt")?;
        let lead = offset + 1;
        hash(row, "sourceRowSha256")?;
        // reject missing, reordered or already valid leads
        if unsigned(row, "leadHours")? != lead as u64
            || valid_at <= issued
            || previous.is_some_and(|last| valid_at.checked_sub(last) != Some(HOUR_MS))
        {
            return None;
        }
        previous = Some(valid_at);
        validate_body_row(family, row, lead)?;
    }
    Some(())
}

/// Check the exact family metric and fallback rules on one known row.
fn validate_body_row(family: ShadowFamily, row: &JsonValue, lead: usize) -> Option<()> {
    match family {
        ShadowFamily::Temperature => {
            binary64_in(row, "candidateTemperatureC64", -100.0, 70.0, false)?;
            validate_fallback(row)
        }
        ShadowFamily::Rain => {
            // bound each raw probability without introducing normalization
            for key in [
                "occurrenceProbability64",
                "atLeast1_0Probability64",
                "atLeast2_5Probability64",
            ] {
                binary64_in(row, key, 0.0, 1.0, false)?;
            }
            // keep both amount fields within the same frozen cap
            for key in ["positiveAmountMm64", "candidatePrecipitationMm64"] {
                binary64_in(row, key, 0.0, 30.0, false)?;
            }
            validate_fallback(row)
        }
        ShadowFamily::Wind => {
            binary64_in(row, "candidateSpeedMps64", 0.0, 150.0, false)?;
            let (JsonValue::Boolean(_), JsonValue::Boolean(gust_apply)) = (
                field(row, "speedWouldApply")?,
                field(row, "gustWouldApply")?,
            ) else {
                return None;
            };
            // only null and false can encode the disabled gust interval
            if (49..=72).contains(&lead) {
                return (matches!(field(row, "candidateGustMps64"), Some(JsonValue::Null))
                    && !gust_apply)
                    .then_some(());
            }
            binary64_in(row, "candidateGustMps64", 0.0, 150.0, false)
        }
    }
}

/// Keep each permitted fallback in agreement with its application flag.
fn validate_fallback(row: &JsonValue) -> Option<()> {
    let JsonValue::Boolean(apply) = field(row, "wouldApply")? else {
        return None;
    };
    match text(row, "fallbackCode")?.as_str() {
        "none" if *apply => Some(()),
        "missing_source" | "ineligible" | "physical_cap" | "model_unavailable" if !apply => {
            Some(())
        }
        _ => None,
    }
}

/// Require the closed object member set in its original serialization order.
pub(crate) fn ordered_keys(doc: &JsonValue, keys: &[&str]) -> Option<()> {
    let JsonValue::Object(fields) = doc else {
        return None;
    };
    (keys.len() == fields.len()
        && fields
            .iter()
            .zip(keys)
            .all(|((member, _), key)| member.iter().copied().eq(key.encode_utf16())))
    .then_some(())
}

/// Borrow one required array member.
pub(crate) fn rows<'a>(doc: &'a JsonValue, name: &str) -> Option<&'a [JsonValue]> {
    let JsonValue::Array(rows) = field(doc, name)? else {
        return None;
    };
    Some(rows)
}

/// Borrow a lowercase canonical digest without allocating a new copy.
pub(crate) fn hash<'a>(doc: &'a JsonValue, key: &str) -> Option<&'a [u16]> {
    let JsonValue::String(hash) = field(doc, key)? else {
        return None;
    };
    (hash.len() == 64
        && hash.iter().all(|unit| {
            (u16::from(b'0')..=u16::from(b'9')).contains(unit)
                || (u16::from(b'a')..=u16::from(b'f')).contains(unit)
        }))
    .then_some(hash.as_slice())
}

/// Resolve a four-digit-year exact millisecond UTC instant through the approved calendar.
pub(crate) fn instant(
    values: &impl ApiMaintenanceReceiptCanonicalValues,
    doc: &JsonValue,
    key: &str,
) -> Option<i64> {
    let time = text(doc, key)?;
    // the original contract excludes extended-year representations
    if time.len() != 24 || !time.as_bytes()[..4].iter().all(u8::is_ascii_digit) {
        return None;
    }
    values.canonical_utc_epoch_milliseconds(&time)
}

/// Admit an independent fixed-cadence due identity and its real calendar date.
pub(crate) fn due_key(
    values: &impl ApiMaintenanceReceiptCanonicalValues,
    doc: &JsonValue,
) -> Option<()> {
    let key = text(doc, "dueKey")?;
    capture_cycle(&key)?;
    values.canonical_utc_epoch_milliseconds(key.strip_prefix("capture/")?)?;
    Some(())
}

/// Read a nonnegative JavaScript-safe number without string coercion.
pub(crate) fn unsigned(doc: &JsonValue, key: &str) -> Option<u64> {
    let JsonValue::Number(value) = field(doc, key)? else {
        return None;
    };
    ((0.0..=9_007_199_254_740_991.0).contains(value) && value.fract() == 0.0)
        .then_some(*value as u64)
}

/// Admit one frozen binary64 field, using Rust's integer and IEEE bit conversions.
pub(crate) fn binary64_in(
    doc: &JsonValue,
    key: &str,
    min: f64,
    max: f64,
    nullable: bool,
) -> Option<()> {
    let value = field(doc, key)?;
    // preserve exact explicit gaps when permitted
    if nullable && value == &JsonValue::Null {
        return Some(());
    }
    let JsonValue::String(encoded) = value else {
        return None;
    };
    let mut ascii = [0; 16];
    // accept only the original lowercase fixed-width network bits
    if encoded.len() != ascii.len() {
        return None;
    }
    // copy the 16 hex digits without allocating a string
    for (byte, unit) in ascii.iter_mut().zip(encoded) {
        *byte = u8::try_from(*unit).ok()?;
        // rule out nonhex and uppercase before integer decoding
        if !byte.is_ascii_digit() && !(b'a'..=b'f').contains(byte) {
            return None;
        }
    }
    let decoded = f64::from_bits(u64::from_str_radix(std::str::from_utf8(&ascii).ok()?, 16).ok()?);
    (min..=max).contains(&decoded).then_some(())
}

/// Check original length and control bounds without replacing JavaScript UTF-16 code units.
pub(crate) fn bounded_source_string(value: &JsonValue) -> Option<&[u16]> {
    let JsonValue::String(units) = value else {
        return None;
    };
    ((1..=128).contains(&units.len()) && units.iter().all(|unit| *unit > 31 && *unit != 127))
        .then_some(units)
}
