//! Bind retained JSON decoding to the generated scheduler slot projection.

use weather_contracts::{
    AdjustmentShadowFamily, AdjustmentShadowRegistrationSlot,
    AdjustmentShadowRegistrationSlotContractVersionLiteral,
    AdjustmentShadowRegistrationSlotStateEnum, JsonValue, Nullable,
};

use crate::{ForecastEvidenceMaterial, ForecastEvidenceReceiptCodec};

use super::forecast_scheduler_admission::ForecastSchedulerSlotDecoder;

const SLOT_KEYS: [&str; 8] = [
    "contractVersion",
    "epochWitnessSha256",
    "family",
    "horizonEndAt",
    "registrationSha256",
    "scheduleContractSha256",
    "state",
    "terminalAt",
];

impl<Codec: ForecastEvidenceReceiptCodec> ForecastSchedulerSlotDecoder
    for ForecastEvidenceMaterial<Codec>
{
    /// Decode only the exact generated slot array through the admitted retained JSON owner.
    fn decode_slots(&self, body: &[u8]) -> Option<Vec<AdjustmentShadowRegistrationSlot>> {
        let JsonValue::Array(slots) = self.codec().decode_retained_json(body)? else {
            return None;
        };
        slots.iter().map(decode_slot).collect()
    }
}

type Object = [(Vec<u16>, JsonValue)];

// project one exact eight-field object into the generated registration slot
fn decode_slot(value: &JsonValue) -> Option<AdjustmentShadowRegistrationSlot> {
    let JsonValue::Object(fields) = value else {
        return None;
    };

    // reject every missing, duplicate, or unknown source key before reading values
    if !has_exact_slot_keys(fields) {
        return None;
    }
    let contract_version = text(fields, "contractVersion")?;

    // admit only the generated v3 literal
    if contract_version != AdjustmentShadowRegistrationSlotContractVersionLiteral::WIRE_VALUE {
        return None;
    }
    Some(AdjustmentShadowRegistrationSlot {
        contractVersion: AdjustmentShadowRegistrationSlotContractVersionLiteral,
        epochWitnessSha256: text(fields, "epochWitnessSha256")?,
        family: decode_family(&text(fields, "family")?)?,
        horizonEndAt: text(fields, "horizonEndAt")?,
        registrationSha256: nullable_text(fields, "registrationSha256")?,
        scheduleContractSha256: text(fields, "scheduleContractSha256")?,
        state: decode_state(&text(fields, "state")?)?,
        terminalAt: nullable_text(fields, "terminalAt")?,
    })
}

// bind the original UTF-16 object keys to one closed source key set
fn has_exact_slot_keys(fields: &Object) -> bool {
    // require the exact source cardinality before detecting each key once
    if fields.len() != SLOT_KEYS.len() {
        return false;
    }
    let mut seen = [false; SLOT_KEYS.len()];

    // assign each original field to one generated wire key
    for (name, _) in fields {
        let Some(index) = SLOT_KEYS.iter().position(
            // compare without normalizing or allocating the original UTF-16 spelling
            |expected| name.iter().copied().eq(expected.encode_utf16()),
        ) else {
            return false;
        };

        // reject duplicate JSON member names even if the admitted decoder preserves them
        if seen[index] {
            return false;
        }
        seen[index] = true;
    }
    seen.into_iter().all(|present| present)
}

// recover one required source string without admitting numeric or null aliases
fn text(fields: &Object, key: &str) -> Option<String> {
    let (_, JsonValue::String(value)) = fields.iter().find(
        // compare the exact original UTF-16 key spelling
        |(name, _)| name.iter().copied().eq(key.encode_utf16()),
    )?
    else {
        return None;
    };
    String::from_utf16(value).ok()
}

// preserve an exact source null or string in the generated nullable projection
fn nullable_text(fields: &Object, key: &str) -> Option<Nullable<String>> {
    let (_, value) = fields.iter().find(
        // compare the exact original UTF-16 key spelling
        |(name, _)| name.iter().copied().eq(key.encode_utf16()),
    )?;
    match value {
        JsonValue::Null => Some(Nullable::Null),
        JsonValue::String(value) => Some(Nullable::Value(String::from_utf16(value).ok()?)),
        _ => None,
    }
}

// decode one generated family wire value without accepting aliases
fn decode_family(value: &str) -> Option<AdjustmentShadowFamily> {
    AdjustmentShadowFamily::WIRE_VALUES.iter().find_map(
        // return the generated variant paired with the exact wire spelling
        |(wire, family)| (*wire == value).then_some(*family),
    )
}

// decode one generated scheduler occupancy state without accepting aliases
fn decode_state(value: &str) -> Option<AdjustmentShadowRegistrationSlotStateEnum> {
    AdjustmentShadowRegistrationSlotStateEnum::WIRE_VALUES
        .iter()
        .find_map(
            // return the generated variant paired with the exact wire spelling
            |(wire, state)| (*wire == value).then_some(*state),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ForecastEvidenceMaterialCodec, RevisionArchiveCanonicalCodec};

    #[derive(Clone)]
    struct FixtureCodec {
        decoded: JsonValue,
    }

    impl RevisionArchiveCanonicalCodec for FixtureCodec {
        /// Keep unrelated time parsing outside this structural decoder fixture.
        fn canonical_utc_milliseconds(&self, _text: &str) -> Option<i64> {
            None
        }

        /// Keep unrelated hashing outside this structural decoder fixture.
        fn sha256(&self, _bytes: &[u8]) -> [u8; 32] {
            [0; 32]
        }
    }

    impl ForecastEvidenceMaterialCodec for FixtureCodec {
        /// Reject unrelated projected forecast decoding.
        fn decode_projected_forecast(&self, _bytes: &[u8]) -> Option<JsonValue> {
            None
        }

        /// Reject unrelated canonical JSON encoding.
        fn canonical_json(&self, _value: &JsonValue) -> Option<Vec<u8>> {
            None
        }

        /// Reject unrelated gzip encoding.
        fn gzip_level_nine_zero_mtime(&self, _bytes: &[u8]) -> Option<Vec<u8>> {
            None
        }

        /// Reject unrelated base64 encoding.
        fn standard_base64(&self, _bytes: &[u8]) -> Option<String> {
            None
        }
    }

    impl ForecastEvidenceReceiptCodec for FixtureCodec {
        /// Return the exact typed fixture only for the admitted body.
        fn decode_retained_json(&self, bytes: &[u8]) -> Option<JsonValue> {
            (bytes == b"fixture").then(|| self.decoded.clone())
        }

        /// Reject unrelated base64 decoding.
        fn decode_standard_base64(&self, _value: &str) -> Option<Vec<u8>> {
            None
        }

        /// Reject unrelated gzip decoding.
        fn gunzip_bounded(&self, _bytes: &[u8], _maximum_bytes: usize) -> Option<Vec<u8>> {
            None
        }
    }

    #[test]
    fn decodes_exact_generated_literals_and_nullable_fields() {
        let material = ForecastEvidenceMaterial::new(FixtureCodec {
            decoded: JsonValue::Array(vec![
                slot("temperature", "free", JsonValue::Null, JsonValue::Null),
                slot(
                    "wind",
                    "busy_v3",
                    string(&"b".repeat(64)),
                    string("2028-10-08T06:00:00.000Z"),
                ),
                slot(
                    "rain",
                    "busy_v2_legacy",
                    string(&"c".repeat(64)),
                    string("2028-10-08T06:30:00.000Z"),
                ),
            ]),
        });
        let decoded = material.decode_slots(b"fixture").expect("decode slots");
        assert_eq!(decoded.len(), 3);
        assert_eq!(decoded[0].family, AdjustmentShadowFamily::Temperature);
        assert_eq!(
            decoded[1].state,
            AdjustmentShadowRegistrationSlotStateEnum::BusyV3
        );
        assert_eq!(
            decoded[2].registrationSha256,
            Nullable::Value("c".repeat(64))
        );
        assert_eq!(decoded[0].terminalAt, Nullable::Null);
    }

    #[test]
    fn rejects_unknown_duplicate_and_wrong_generated_values() {
        let mut unknown = slot("temperature", "free", JsonValue::Null, JsonValue::Null);
        let JsonValue::Object(fields) = &mut unknown else {
            panic!("slot fixture is an object");
        };
        fields[0].0 = "unknown".encode_utf16().collect();

        let mut duplicate = slot("temperature", "free", JsonValue::Null, JsonValue::Null);
        let JsonValue::Object(fields) = &mut duplicate else {
            panic!("slot fixture is an object");
        };
        fields[0].0 = "contractVersion".encode_utf16().collect();

        // reject each independent closed-shape or generated-enum violation
        for invalid in [
            unknown,
            duplicate,
            slot("snow", "free", JsonValue::Null, JsonValue::Null),
            slot("rain", "busy", JsonValue::Null, JsonValue::Null),
        ] {
            let material = ForecastEvidenceMaterial::new(FixtureCodec {
                decoded: JsonValue::Array(vec![invalid]),
            });
            assert!(material.decode_slots(b"fixture").is_none());
        }
    }

    // construct one exact eight-field source slot in deliberately non-sorted order
    fn slot(
        family: &str,
        state: &str,
        registration_sha256: JsonValue,
        terminal_at: JsonValue,
    ) -> JsonValue {
        object([
            ("state", string(state)),
            (
                "contractVersion",
                string(AdjustmentShadowRegistrationSlotContractVersionLiteral::WIRE_VALUE),
            ),
            ("terminalAt", terminal_at),
            ("family", string(family)),
            ("registrationSha256", registration_sha256),
            ("epochWitnessSha256", string(&"a".repeat(64))),
            ("horizonEndAt", string("2028-10-08T07:00:00.000Z")),
            (
                "scheduleContractSha256",
                string("7c17f5d1a8e8249cd0aa4820638169e51f6edb3433017f50ab4c959e44c62f1f"),
            ),
        ])
    }

    // construct one typed fixture object without implementing JSON parsing
    fn object<const N: usize>(fields: [(&str, JsonValue); N]) -> JsonValue {
        JsonValue::Object(
            fields
                .into_iter()
                .map(
                    // retain each exact source UTF-16 key spelling
                    |(key, value)| (key.encode_utf16().collect(), value),
                )
                .collect(),
        )
    }

    // construct one typed fixture string without implementing JSON parsing
    fn string(value: &str) -> JsonValue {
        JsonValue::String(value.encode_utf16().collect())
    }
}
