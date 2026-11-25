//! Compact ordinary parity coverage for the three frozen shadow body wires.

use weather_contracts::JsonValue;
use weather_database::Sha256Digest;

use crate::api_shadow_wire_validation::validate_shadow_body;
use crate::{ApiMaintenanceReceiptCanonicalValues, MaintenanceReceiptScalar};

const HASH: &str = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
const ISSUED_AT: &str = "2026-11-01T00:35:00.000Z";
const DUE_KEY: &str = "capture/2026-11-01T00:35:00.000Z";
const TEMPERATURE_MINIMUM: &str = "c059000000000000";
const TEMPERATURE_TOO_HIGH: &str = "4051c00000000000";
const PROBABILITY_ONE: &str = "3ff0000000000000";
const RAIN_MAXIMUM: &str = "403e000000000000";
const WIND_MAXIMUM: &str = "4062c00000000000";

#[derive(Clone, Copy)]
enum Family {
    Temperature,
    Wind,
    Rain,
}

impl Family {
    /// Return the original family wire spelling.
    const fn name(self) -> &'static str {
        match self {
            Self::Temperature => "temperature",
            Self::Wind => "wind",
            Self::Rain => "rain",
        }
    }

    /// Return the original fixed family geometry.
    const fn row_count(self) -> usize {
        match self {
            Self::Temperature => 12,
            Self::Wind => 168,
            Self::Rain => 23,
        }
    }

    /// Return the original encoded body ceiling.
    const fn byte_limit(self) -> usize {
        match self {
            Self::Temperature => 8_192,
            Self::Wind => 65_536,
            Self::Rain => 12_288,
        }
    }
}

struct Values {
    clocks: Vec<String>,
}

impl Values {
    /// Retain every canonical clock needed by the largest frozen geometry.
    fn new() -> Self {
        Self {
            clocks: (1..=Family::Wind.row_count()).map(valid_at).collect(),
        }
    }
}

impl ApiMaintenanceReceiptCanonicalValues for Values {
    /// Recognize only the fixture's exact millisecond UTC clock spellings.
    fn canonical_utc_epoch_milliseconds(&self, value: &str) -> Option<i64> {
        match value {
            ISSUED_AT => Some(0),
            _ => self
                .clocks
                .iter()
                .position(|clock| clock == value)
                .and_then(|index| i64::try_from(index + 1).ok())
                .map(|lead| lead * 3_600_000),
        }
    }

    /// Wire validation must not derive a new digest.
    fn sha256(&self, _: &[u8]) -> Option<Sha256Digest> {
        unreachable!("shadow wire validation must not hash bytes")
    }

    /// Wire validation must not encode a receipt.
    fn unsigned_receipt_sha256(
        &self,
        _: &[(&str, MaintenanceReceiptScalar<'_>)],
    ) -> Option<Sha256Digest> {
        unreachable!("shadow wire validation must not encode receipts")
    }
}

/// Build one ordered dependency-free JSON object.
fn object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(name, value)| (name.encode_utf16().collect(), value))
            .collect(),
    )
}

/// Build one dependency-free JSON string.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Build one exactly representable nonnegative fixture integer.
fn number(value: usize) -> JsonValue {
    JsonValue::Number(f64::from(
        u32::try_from(value).expect("fixture integers must fit u32"),
    ))
}

/// Render one consecutive hourly valid clock without a date package.
fn valid_at(lead: usize) -> String {
    let day = 1 + lead / 24;
    let hour = lead % 24;
    format!("2026-11-{day:02}T{hour:02}:35:00.000Z")
}

/// Build one exact family row in the original TypeScript key order.
fn row(family: Family, lead: usize) -> JsonValue {
    let valid_at = valid_at(lead);
    let lead_number = number(lead);
    match family {
        Family::Temperature => object(vec![
            ("validAt", string(&valid_at)),
            ("leadHours", lead_number),
            ("sourceRowSha256", string(HASH)),
            ("candidateTemperatureC64", string(TEMPERATURE_MINIMUM)),
            ("wouldApply", JsonValue::Boolean(false)),
            ("fallbackCode", string("model_unavailable")),
        ]),
        Family::Wind => {
            let disabled_gust = (49..=72).contains(&lead);
            object(vec![
                ("validAt", string(&valid_at)),
                ("leadHours", lead_number),
                ("sourceRowSha256", string(HASH)),
                ("candidateSpeedMps64", string(WIND_MAXIMUM)),
                (
                    "candidateGustMps64",
                    // preserve Node's structural gust outage
                    if disabled_gust {
                        JsonValue::Null
                    } else {
                        string(WIND_MAXIMUM)
                    },
                ),
                ("speedWouldApply", JsonValue::Boolean(false)),
                ("gustWouldApply", JsonValue::Boolean(false)),
            ])
        }
        Family::Rain => object(vec![
            ("validAt", string(&valid_at)),
            ("leadHours", lead_number),
            ("sourceRowSha256", string(HASH)),
            ("occurrenceProbability64", string(PROBABILITY_ONE)),
            ("positiveAmountMm64", string(RAIN_MAXIMUM)),
            ("candidatePrecipitationMm64", string(RAIN_MAXIMUM)),
            ("atLeast1_0Probability64", string(PROBABILITY_ONE)),
            ("atLeast2_5Probability64", string(PROBABILITY_ONE)),
            ("wouldApply", JsonValue::Boolean(false)),
            ("fallbackCode", string("model_unavailable")),
        ]),
    }
}

/// Build one complete body in the original common-key order.
fn body(family: Family) -> JsonValue {
    let rows = (1..=family.row_count())
        .map(|lead| row(family, lead))
        .collect();
    object(vec![
        (
            "contractVersion",
            string("adjustment-shadow-prediction-values/v3"),
        ),
        ("family", string(family.name())),
        ("registrationSha256", string(HASH)),
        ("candidateSha256", string(HASH)),
        ("sourceSha256", string(HASH)),
        ("dueKey", string(DUE_KEY)),
        ("issuedAt", string(ISSUED_AT)),
        ("sourceReceiptSha256", string(HASH)),
        ("inputSha256", string(HASH)),
        ("rowCount", number(family.row_count())),
        ("rows", JsonValue::Array(rows)),
    ])
}

/// Replace one known row field while retaining its original position.
fn replace_row_field(body: &mut JsonValue, row_index: usize, key: &str, replacement: JsonValue) {
    let JsonValue::Object(body_fields) = body else {
        panic!("fixture body must be an object");
    };
    let JsonValue::Array(rows) = body_fields
        .iter_mut()
        .find_map(|(name, value)| {
            name.iter()
                .copied()
                .eq("rows".encode_utf16())
                .then_some(value)
        })
        .expect("fixture body must retain rows")
    else {
        panic!("fixture rows must be an array");
    };
    let JsonValue::Object(row_fields) = rows
        .get_mut(row_index)
        .expect("fixture row index must be in range")
    else {
        panic!("fixture row must be an object");
    };
    let value = row_fields
        .iter_mut()
        .find_map(|(name, value)| name.iter().copied().eq(key.encode_utf16()).then_some(value))
        .expect("fixture row must retain the requested field");
    *value = replacement;
}

#[test]
fn all_three_frozen_families_fit_only_their_original_geometry_and_byte_ceiling() {
    let values = Values::new();
    // prove the three independent family limits selected by the compiled Node fixture
    for family in [Family::Temperature, Family::Wind, Family::Rain] {
        let body = body(family);
        assert_eq!(
            validate_shadow_body(&values, &body, family.byte_limit()),
            Some(())
        );
        assert_eq!(
            validate_shadow_body(&values, &body, family.byte_limit() + 1),
            None
        );
    }
}

#[test]
fn wind_retains_the_exact_disabled_gust_interval_boundaries() {
    let values = Values::new();
    // leads 48 and 73 remain enabled while 49 through 72 remain disabled
    for (lead, replacement) in [
        (48, JsonValue::Null),
        (49, string(WIND_MAXIMUM)),
        (72, string(WIND_MAXIMUM)),
        (73, JsonValue::Null),
    ] {
        let mut body = body(Family::Wind);
        replace_row_field(&mut body, lead - 1, "candidateGustMps64", replacement);
        assert_eq!(
            validate_shadow_body(&values, &body, Family::Wind.byte_limit()),
            None
        );
    }
}

#[test]
fn family_values_fallbacks_and_clocks_match_the_compiled_node_choices() {
    let values = Values::new();

    let mut temperature = body(Family::Temperature);
    replace_row_field(
        &mut temperature,
        0,
        "candidateTemperatureC64",
        string(TEMPERATURE_TOO_HIGH),
    );
    assert_eq!(
        validate_shadow_body(&values, &temperature, Family::Temperature.byte_limit()),
        None
    );

    let mut rain = body(Family::Rain);
    replace_row_field(&mut rain, 0, "fallbackCode", string("none"));
    assert_eq!(
        validate_shadow_body(&values, &rain, Family::Rain.byte_limit()),
        None
    );

    let mut shifted_clock = body(Family::Temperature);
    replace_row_field(&mut shifted_clock, 1, "validAt", string(&valid_at(3)));
    assert_eq!(
        validate_shadow_body(&values, &shifted_clock, Family::Temperature.byte_limit()),
        None
    );
}
