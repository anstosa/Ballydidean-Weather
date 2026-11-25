//! Full producer-side projection admission, separate from retained cold reads.

use weather_contracts::JsonValue;

use crate::archive_wire::{ArchiveWireCodec, ArchiveWirePrimitives, field, member, text};
use crate::private_revision_archive::{
    AdmittedPrivateRevisionProjection, PrivateRevisionArchiveCodec,
    PrivateRevisionProjectionDependency,
};
use crate::rain_control_state_archive::{
    RainControlProjectionBinding, RainControlStateArchiveCodec,
};
use crate::revision_archive_canonical::{RevisionArchiveCanonicalCodec, instant};

const ORDINARY_MAXIMUM_BYTES: usize = 768 * 1024;
const FIXED_GAUGE_MAXIMUM_BYTES: usize = 3_500 * 1024;

/// Admit the complete original producer projection, before any archive write.
pub fn admit_archive_revision_projection<P: ArchiveWirePrimitives>(
    codec: &ArchiveWireCodec<P>,
    bytes: &[u8],
) -> Option<AdmittedPrivateRevisionProjection> {
    // reject invalid UTF-8 and oversized documents before decoding their value tree
    if !(2..=FIXED_GAUGE_MAXIMUM_BYTES).contains(&bytes.len()) {
        return None;
    }
    std::str::from_utf8(bytes).ok()?;
    let value = codec.decode(bytes)?;
    let contract = text(&value, "contractVersion")?;
    // preserve the independently larger fixed-gauge representation only on its exact grammar
    if contract == "adjustment-rain-fixed-gauge-target-projection/v1" {
        return crate::producer_projection_fixed_gauge::admit_fixed_gauge_projection(
            codec, &value, bytes,
        );
    }
    if bytes.len() > ORDINARY_MAXIMUM_BYTES {
        return None;
    }
    match contract.as_str() {
        "adjustment-revision-projection/v1"
        | "adjustment-revision-batch-projection/v2"
        | "adjustment-temperature-native-source-projection/v2" => {
            crate::producer_projection_legacy::admit_legacy_projection(codec, &value, bytes)
        }
        "adjustment-rain-gate-feature-projection/v2"
        | "adjustment-rain-gate-control-projection/v3" => {
            crate::producer_projection_rain::admit_rain_projection(codec, &value, bytes)
        }
        _ => None,
    }
}

impl<P: ArchiveWirePrimitives> PrivateRevisionArchiveCodec for ArchiveWireCodec<P> {
    /// Bind the actual stage writer to the complete producer grammars.
    fn decode_projection(&self, bytes: &[u8]) -> Option<AdmittedPrivateRevisionProjection> {
        admit_archive_revision_projection(self, bytes)
    }
}

impl<P: ArchiveWirePrimitives> RainControlStateArchiveCodec for ArchiveWireCodec<P> {
    /// Require full producer admission before deciding whether a projection references a state.
    fn decode_projection(&self, bytes: &[u8]) -> Option<RainControlProjectionBinding> {
        let projection = admit_archive_revision_projection(self, bytes)?;
        Some(match projection.dependency {
            PrivateRevisionProjectionDependency::RainControlState { state_sha256, .. } => {
                RainControlProjectionBinding::RainControl(state_sha256)
            }
            PrivateRevisionProjectionDependency::None => RainControlProjectionBinding::Other,
        })
    }
}

/// Require the original four-digit millisecond UTC clock before calendar admission.
pub(crate) fn instant_field(
    codec: &impl RevisionArchiveCanonicalCodec,
    value: &JsonValue,
    name: &str,
) -> Option<i64> {
    let clock = text(value, name)?;
    // retain exactly the producer's four-digit UTC representation
    if clock.len() != 24 {
        return None;
    }
    instant(codec, &clock)
}

/// Decode one exact finite binary64 wire value without decimal conversion.
pub(crate) fn binary64(value: &JsonValue) -> Option<f64> {
    let JsonValue::String(units) = value else {
        return None;
    };
    let text = String::from_utf16(units).ok()?;
    // admit precisely the original sixteen lowercase hexadecimal code units
    if text.len() != 16
        || !text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return None;
    }
    let number = f64::from_bits(u64::from_str_radix(&text, 16).ok()?);
    number.is_finite().then_some(number)
}

/// Admit a finite binary64 or an explicit original null.
pub(crate) fn nullable_binary64(value: &JsonValue) -> Option<()> {
    match value {
        JsonValue::Null => Some(()),
        _ => binary64(value).map(|_| ()),
    }
}

/// Admit one exact bounded safe JavaScript integer.
pub(crate) fn integer(value: &JsonValue, min: i64, max: i64) -> Option<i64> {
    let JsonValue::Number(value) = value else {
        return None;
    };
    // enforce integer, safe-number and caller-selected bounds independently
    if !value.is_finite()
        || value.fract() != 0.0
        || value.abs() > 9_007_199_254_740_991.0
        || *value < min as f64
        || *value > max as f64
    {
        return None;
    }
    Some(*value as i64)
}

/// Admit the source's bounded nonempty lineage or reason string by UTF-16 units.
pub(crate) fn bounded_text(value: &JsonValue) -> bool {
    matches!(value,JsonValue::String(units) if (1..=128).contains(&units.len())
        && units.iter().all(|unit| *unit>0x1f && *unit!=0x7f))
}

/// Copy one already validated object's exact producer field order.
pub(crate) fn ordered(value: &JsonValue, keys: &[&str]) -> Option<JsonValue> {
    let entries = keys
        .iter()
        .map(
            // preserve each original value and its fixed output order
            |key| Some(member(key, field(value, key)?.clone())),
        )
        .collect::<Option<Vec<_>>>()?;
    Some(JsonValue::Object(entries))
}

/// Return the exact source cap of each producer projection family.
pub(crate) fn family_limit(family: &str) -> Option<usize> {
    Some(match family {
        "rain" => 128 * 1024,
        "temperature" => 64 * 1024,
        "shared" | "wind" => ORDINARY_MAXIMUM_BYTES,
        _ => return None,
    })
}
