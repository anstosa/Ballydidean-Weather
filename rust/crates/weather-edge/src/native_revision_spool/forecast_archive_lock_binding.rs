//! Use the admitted JSON and calendar owners for the shared archive lease.

use weather_contracts::JsonValue;

use crate::ForecastEvidenceReceiptCodec;
use crate::forecast_scheduler_due::ForecastSchedulerCalendarPort;
use crate::revision_archive_canonical::{
    RevisionArchiveCanonicalClock, RevisionArchiveCanonicalCodec, instant,
};

use super::forecast_archive_lock::{ForecastArchiveLockClock, ForecastArchiveLockCodec};

const VERSION: &str = "adjustment-archive-lock/v1";

/// Small binding for the exact canonical JSON/UTC archive transport lock.
pub struct BoundForecastArchiveLockCodec<'a, Codec, Calendar> {
    bytes: &'a Codec,
    calendar: &'a Calendar,
}

impl<'a, Codec, Calendar> BoundForecastArchiveLockCodec<'a, Codec, Calendar> {
    /// Reuse the current runtime's existing exact codec and canonical clock formatter.
    pub const fn new(bytes: &'a Codec, calendar: &'a Calendar) -> Self {
        Self { bytes, calendar }
    }
}

impl<Codec: ForecastEvidenceReceiptCodec, Calendar: ForecastSchedulerCalendarPort>
    ForecastArchiveLockCodec for BoundForecastArchiveLockCodec<'_, Codec, Calendar>
{
    /// Encode the three existing lease members with the predecessor's newline framing.
    fn encode_forecast_archive_lease(&self, expires_at: i64, token: &str) -> Option<Vec<u8>> {
        let formatted = self.calendar.canonical_utc(expires_at)?;
        // round-trip the exact finite clock and the Node token grammar
        if instant(self.bytes, &formatted) != Some(expires_at) || !is_lease_token(token) {
            return None;
        }
        self.frame(&JsonValue::Object(vec![
            field("contractVersion", VERSION),
            field("expiresAt", &formatted),
            field("token", token),
        ]))
    }

    /// Check complete byte identity before honoring a persisted expiry.
    fn decode_forecast_archive_expiry(&self, bytes: &[u8]) -> Option<i64> {
        let value = self.bytes.decode_retained_json(bytes)?;
        let JsonValue::Object(fields) = &value else {
            return None;
        };
        let version = read_field(fields, "contractVersion")?;
        let token = read_field(fields, "token")?;
        let expires = read_field(fields, "expiresAt")?;
        // do not substitute a noncanonical or unrelated lock document
        if version != VERSION || !is_lease_token(&token) || self.frame(&value)? != bytes {
            return None;
        }
        instant(self.bytes, &expires)
    }
}

impl<Codec: ForecastEvidenceReceiptCodec, Calendar>
    BoundForecastArchiveLockCodec<'_, Codec, Calendar>
{
    /// Delegate actual JSON spelling and add only its retained line envelope.
    fn frame(&self, value: &JsonValue) -> Option<Vec<u8>> {
        let mut bytes = self.bytes.canonical_json(value)?;
        bytes.push(b'\n');
        Some(bytes)
    }
}

/// Adapt the admitted UTC observation to the existing expiry comparison port.
pub struct BoundForecastArchiveLockClock<'a, Clock, Codec> {
    clock: &'a mut Clock,
    codec: &'a Codec,
}

impl<'a, Clock, Codec> BoundForecastArchiveLockClock<'a, Clock, Codec> {
    /// Bind the current wall clock to the same canonical UTC parser.
    pub const fn new(clock: &'a mut Clock, codec: &'a Codec) -> Self {
        Self { clock, codec }
    }
}

impl<Clock: RevisionArchiveCanonicalClock, Codec: RevisionArchiveCanonicalCodec>
    ForecastArchiveLockClock for BoundForecastArchiveLockClock<'_, Clock, Codec>
{
    /// Use the exact same runtime clock for initial acquisition and expiry checks.
    fn now_epoch_milliseconds(&mut self) -> Option<i64> {
        instant(self.codec, &self.clock.canonical_now()?)
    }
}

/// Represent a known lease field inside the existing generated value type.
fn field(name: &str, value: &str) -> (Vec<u16>, JsonValue) {
    (
        name.encode_utf16().collect(),
        JsonValue::String(value.encode_utf16().collect()),
    )
}

/// Access one retained lease field without implementing JSON decoding.
fn read_field(fields: &[(Vec<u16>, JsonValue)], name: &str) -> Option<String> {
    let (_, JsonValue::String(text)) = fields
        .iter()
        .find(|(key, _)| key.iter().copied().eq(name.encode_utf16()))?
    else {
        return None;
    };
    String::from_utf16(text).ok()
}

/// Match the exact existing canonical lease token alphabet and extent.
fn is_lease_token(value: &str) -> bool {
    value.len() == 36
        && value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f' | b'-'))
}
