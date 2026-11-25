//! Native two-stage Ecowitt gateway transport, calendar, and metadata owners.

use std::sync::{Arc, Mutex};

use jiff::{Timestamp, tz::TimeZone};
use url::Url;
use weather_contracts::JsonValue;
use weather_database::Sha256Digest;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};
use weather_providers::{
    ProviderAttempt, ProviderAttemptError, ProviderAttemptResponse, ProviderIo, ProviderSleep,
    ProviderSleepError,
};
use weather_worker::{
    EcowittAdmittedLocalDate, EcowittBatteryState, EcowittCanonicalMetadataAdapter,
    EcowittCanonicalMetadataMeasurement, EcowittCommonReadings, EcowittGatewayReadings,
    EcowittLivePayload, EcowittMeasurement, EcowittParticulateSensor, EcowittProviderMetadata,
    EcowittRainReadings, EcowittScheduledRequest, EcowittSoilSensor, EcowittTemperatureSensor,
    EcowittTimeAdapter, EcowittUnit, TaskFailure,
};

use crate::maintenance_canonical_json::{field, text};
use crate::native_open_meteo_transport::{decode_provider_json, json_get};
use crate::native_outbound_http::{
    NativeOutboundHttp, NativeOutboundHttpError, NativeOutboundHttpRequest,
    NativeOutboundHttpResponse, NativeProviderCodec, OwnedNativeProviderExchange,
};
use crate::{
    EcowittAttemptPayload, EcowittParsedProduct, EcowittProviderIo, EcowittProviderStage,
    NativeWirePrimitives, WorkerEcowittExchange, WorkerEcowittTransport, ecowitt_provider_metadata,
    load_named_timezone, system_worker_health_milliseconds,
};

const NETWORK_PATH: &str = "/get_network_info";
const LIVE_PATH: &str = "/get_livedata_info";
const MAXIMUM_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

/// Production Ecowitt owner sharing the process outbound runtime.
pub struct NativeEcowittTransport {
    client: Arc<NativeOutboundHttp>,
    #[cfg(test)]
    test_origin: Option<Url>,
}

impl NativeEcowittTransport {
    /// Start one retained pooled client for local gateway requests.
    pub fn new() -> Result<Self, NativeOutboundHttpError> {
        NativeOutboundHttp::new().map(|client| Self::with_client(Arc::new(client)))
    }

    /// Adopt the process client without creating another runtime or pool.
    pub const fn with_client(client: Arc<NativeOutboundHttp>) -> Self {
        Self {
            client,
            #[cfg(test)]
            test_origin: None,
        }
    }

    /// Route fixture requests to loopback without weakening production LAN validation.
    #[cfg(test)]
    pub(crate) const fn with_test_origin(client: Arc<NativeOutboundHttp>, origin: Url) -> Self {
        Self {
            client,
            test_origin: Some(origin),
        }
    }
}

impl WorkerEcowittTransport for NativeEcowittTransport {
    type Io<'a>
        = NativeEcowittIo
    where
        Self: 'a;
    type Time<'a>
        = NativeEcowittTime
    where
        Self: 'a;
    type Metadata<'a>
        = NativeEcowittMetadata
    where
        Self: 'a;

    /// Bind independently selected identity/live requests and exact auxiliary owners.
    fn bind_request(
        &mut self,
        request: &EcowittScheduledRequest,
    ) -> Result<WorkerEcowittExchange<Self::Io<'_>, Self::Time<'_>, Self::Metadata<'_>>, TaskFailure>
    {
        validate_scheduled_request(request)?;
        let timezone = load_named_timezone(&request.timezone).ok_or_else(ecowitt_failure)?;
        let state = Arc::new(Mutex::new(EcowittExchangeState::default()));
        let codec = EcowittCodec {
            state: Arc::clone(&state),
        };
        Ok(WorkerEcowittExchange {
            io: NativeEcowittIo {
                exchange: OwnedNativeProviderExchange::new(Arc::clone(&self.client), codec),
                state,
                #[cfg(test)]
                test_origin: self.test_origin.clone(),
            },
            time: NativeEcowittTime {
                name: request.timezone.clone(),
                timezone,
            },
            metadata: NativeEcowittMetadata,
        })
    }
}

/// Selected multi-stage request state shared with the generic owned exchange codec.
#[derive(Default)]
struct EcowittExchangeState {
    attempt: u8,
    identity_checksum: Option<Sha256Digest>,
    request: Option<NativeOutboundHttpRequest>,
    stage: Option<EcowittProviderStage>,
}

/// Request-selectable IO owner for the two independent provider controllers.
pub struct NativeEcowittIo {
    exchange: OwnedNativeProviderExchange<EcowittCodec>,
    state: Arc<Mutex<EcowittExchangeState>>,
    #[cfg(test)]
    test_origin: Option<Url>,
}

impl EcowittProviderIo for NativeEcowittIo {
    /// Select and validate one exact local endpoint before its retry loop.
    fn select_request(
        &mut self,
        stage: EcowittProviderStage,
        request: &EcowittScheduledRequest,
    ) -> Result<(), TaskFailure> {
        validate_scheduled_request(request)?;
        #[cfg(not(test))]
        let url = gateway_url(
            &request.gateway_host,
            match stage {
                EcowittProviderStage::GatewayIdentity => NETWORK_PATH,
                EcowittProviderStage::LiveData => LIVE_PATH,
            },
        )?;
        #[cfg(test)]
        let url = gateway_url(
            &request.gateway_host,
            match stage {
                EcowittProviderStage::GatewayIdentity => NETWORK_PATH,
                EcowittProviderStage::LiveData => LIVE_PATH,
            },
            self.test_origin.as_ref(),
        )?;
        let outbound = json_get(url).map_err(|_| ecowitt_failure())?;
        let mut state = self.state.lock().map_err(|_| ecowitt_failure())?;
        // start each scheduled request with fresh identity checksum evidence
        if stage == EcowittProviderStage::GatewayIdentity {
            state.identity_checksum = None;
        }
        state.request = Some(outbound);
        state.stage = Some(stage);
        Ok(())
    }
}

impl ProviderIo<EcowittAttemptPayload> for NativeEcowittIo {
    /// Share the retained process clock with the generic native exchange.
    fn now_ms(&mut self) -> i64 {
        self.exchange.now_ms()
    }

    /// Share the retained entropy owner with the generic native exchange.
    fn random_unit(&mut self) -> f64 {
        self.exchange.random_unit()
    }

    /// Execute the selected stage with the controller's exact limits.
    fn execute(
        &mut self,
        attempt: ProviderAttempt,
    ) -> Result<ProviderAttemptResponse<EcowittAttemptPayload>, ProviderAttemptError> {
        self.exchange.execute(attempt)
    }

    /// Sleep only the calling retry controller lane.
    fn sleep(&mut self, sleep: ProviderSleep) -> Result<(), ProviderSleepError> {
        self.exchange.sleep(sleep)
    }
}

/// Shared-state codec for the selected Ecowitt gateway stage.
pub struct EcowittCodec {
    state: Arc<Mutex<EcowittExchangeState>>,
}

impl NativeProviderCodec<EcowittAttemptPayload> for EcowittCodec {
    /// Clone the already-selected request and retain its attempt number.
    fn request(
        &mut self,
        attempt: ProviderAttempt,
    ) -> Result<NativeOutboundHttpRequest, ProviderAttemptError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| transport_attempt_failure(attempt.attempt))?;
        state.attempt = attempt.attempt;
        state
            .request
            .clone()
            .ok_or_else(|| invalid_attempt_failure(attempt.attempt))
    }

    /// Decode the selected identity or live product and retain checksum framing.
    fn decode(
        &mut self,
        response: NativeOutboundHttpResponse,
    ) -> Result<ProviderAttemptResponse<EcowittAttemptPayload>, ProviderAttemptError> {
        let (attempt, stage) = {
            let state = self
                .state
                .lock()
                .map_err(|_| transport_attempt_failure(1))?;
            (
                state.attempt,
                state
                    .stage
                    .ok_or_else(|| invalid_attempt_failure(state.attempt))?,
            )
        };
        let decoded = decode_provider_json(response, attempt)?;
        let product = if (200..300).contains(&decoded.status) {
            match stage {
                EcowittProviderStage::GatewayIdentity => {
                    let gateway_mac = decode_gateway_mac(&decoded.body)
                        .ok_or_else(|| invalid_attempt_failure(attempt))?;
                    self.state
                        .lock()
                        .map_err(|_| transport_attempt_failure(attempt))?
                        .identity_checksum = Some(decoded.checksum.clone());
                    EcowittParsedProduct::GatewayIdentity { gateway_mac }
                }
                EcowittProviderStage::LiveData => {
                    let received_at_ms = system_worker_health_milliseconds().unwrap_or(0);
                    let payload = decode_live_payload(&decoded.body)
                        .ok_or_else(|| invalid_attempt_failure(attempt))?;
                    let identity_checksum = self
                        .state
                        .lock()
                        .map_err(|_| transport_attempt_failure(attempt))?
                        .identity_checksum
                        .clone()
                        .ok_or_else(|| invalid_attempt_failure(attempt))?;
                    EcowittParsedProduct::LiveData {
                        combined_upstream_response_checksum: combined_checksum(
                            &identity_checksum,
                            &decoded.checksum,
                        ),
                        payload: Box::new(payload),
                        received_at_ms,
                    }
                }
            }
        } else {
            match stage {
                EcowittProviderStage::GatewayIdentity => EcowittParsedProduct::GatewayIdentity {
                    gateway_mac: String::new(),
                },
                EcowittProviderStage::LiveData => EcowittParsedProduct::LiveData {
                    combined_upstream_response_checksum: String::new(),
                    payload: Box::default(),
                    received_at_ms: 0,
                },
            }
        };
        Ok(ProviderAttemptResponse {
            body: EcowittAttemptPayload { product },
            content: decoded.content,
            provider_reason: decoded.provider_reason,
            retry_after_ms: decoded.retry_after_ms,
            status: decoded.status,
        })
    }
}

/// Cached timezone owner for source-local rain-day selection.
pub struct NativeEcowittTime {
    name: String,
    timezone: TimeZone,
}

impl EcowittTimeAdapter for NativeEcowittTime {
    /// Resolve one received instant through the admitted host TZif rules.
    fn local_date(
        &mut self,
        received_at_ms: i64,
        timezone: &str,
    ) -> Result<EcowittAdmittedLocalDate, TaskFailure> {
        // refuse a request-specific owner reused for another timezone
        if timezone != self.name {
            return Err(ecowitt_failure());
        }
        let instant = Timestamp::from_millisecond(received_at_ms).map_err(|_| ecowitt_failure())?;
        let date = self.timezone.to_datetime(instant).date();
        Ok(EcowittAdmittedLocalDate::admitted(
            i32::from(date.year()),
            u8::try_from(date.month()).map_err(|_| ecowitt_failure())?,
            u8::try_from(date.day()).map_err(|_| ecowitt_failure())?,
        ))
    }
}

/// Zero-state canonical provider metadata measurement owner.
pub struct NativeEcowittMetadata;

impl EcowittCanonicalMetadataAdapter for NativeEcowittMetadata {
    /// Serialize the actual provider metadata and count JavaScript UTF-16 units.
    fn measure_provider_metadata(
        &mut self,
        metadata: &EcowittProviderMetadata,
    ) -> Result<EcowittCanonicalMetadataMeasurement, TaskFailure> {
        let value = JsonValue::Object(ecowitt_provider_metadata(metadata));
        let mut bytes = NativeWirePrimitives
            .encode_ordered_json(&value)
            .ok_or_else(ecowitt_failure)?;
        // remove native archive framing absent from JSON.stringify
        if bytes.pop() != Some(b'\n') {
            return Err(ecowitt_failure());
        }
        let text = String::from_utf8(bytes).map_err(|_| ecowitt_failure())?;
        Ok(EcowittCanonicalMetadataMeasurement::admitted(
            text.encode_utf16().count(),
        ))
    }
}

/// Construct one stage URL after private-LAN validation.
#[cfg(not(test))]
fn gateway_url(gateway_host: &str, path: &str) -> Result<Url, TaskFailure> {
    Url::parse(&format!("http://{gateway_host}{path}")).map_err(|_| ecowitt_failure())
}

/// Construct one test stage URL while retaining production request validation.
#[cfg(test)]
fn gateway_url(
    gateway_host: &str,
    path: &str,
    test_origin: Option<&Url>,
) -> Result<Url, TaskFailure> {
    // retain production host validation even when the connection target is loopback
    if let Some(origin) = test_origin {
        return origin.join(path).map_err(|_| ecowitt_failure());
    }
    Url::parse(&format!("http://{gateway_host}{path}")).map_err(|_| ecowitt_failure())
}

/// Validate immutable request identity without regex or an extra dependency.
fn validate_scheduled_request(request: &EcowittScheduledRequest) -> Result<(), TaskFailure> {
    // require bounded source identity and a canonical MAC address
    if !bounded_text(&request.model, 128)
        || !bounded_text(&request.source_id, 128)
        || !bounded_text(&request.timezone, 64)
        || !canonical_mac(&request.expected_mac)
        || !private_ipv4(&request.gateway_host)
    {
        return Err(ecowitt_failure());
    }
    Ok(())
}

/// Recognize one canonical colon-separated hardware address.
fn canonical_mac(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 17
        && bytes.iter().enumerate().all(|(index, byte)| {
            // require separators only at octet boundaries
            if matches!(index, 2 | 5 | 8 | 11 | 14) {
                *byte == b':'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

/// Recognize literal RFC1918 dotted quads with bounded decimal octets.
fn private_ipv4(value: &str) -> bool {
    let Some(octets) = parse_ipv4(value) else {
        return false;
    };
    octets[0] == 10
        || (octets[0] == 192 && octets[1] == 168)
        || (octets[0] == 172 && (16..=31).contains(&octets[1]))
}

/// Parse four decimal IPv4 components without accepting signs or whitespace.
fn parse_ipv4(value: &str) -> Option<[u8; 4]> {
    let mut output = [0_u8; 4];
    let mut parts = value.split('.');
    // retain exactly four one-to-three-digit components
    for octet in &mut output {
        let part = parts.next()?;
        if part.is_empty() || part.len() > 3 || !part.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        *octet = part.parse().ok()?;
    }
    parts.next().is_none().then_some(output)
}

/// Decode the gateway's network identity response.
fn decode_gateway_mac(value: &JsonValue) -> Option<String> {
    let mac = text(value, "mac")?;
    (mac.encode_utf16().count() <= 17 && !mac.trim().is_empty()).then_some(mac)
}

/// Decode every supported live-data group into the existing typed boundary.
fn decode_live_payload(root: &JsonValue) -> Option<EcowittLivePayload> {
    let common = required_array(root, "common_list")?;
    let gateway_group = optional_array(root, "wh25")?;
    let temperature = optional_array(root, "ch_aisle")?;
    let soil = optional_array(root, "ch_ec")?;
    let particulate = optional_array(root, "ch_pm25")?;
    let traditional_group = optional_array(root, "rain")?;
    let piezo_group = optional_array(root, "piezoRain")?;
    let common_readings = EcowittCommonReadings {
        apparent_temperature_fallback: entry_measurement(find_id(common, "3"))?,
        apparent_temperature_primary: entry_measurement(find_id(common, "4"))?,
        black_globe_battery: battery(find_id(common, "0xA1")),
        black_globe_temperature: entry_measurement(find_id(common, "0xA1"))?,
        outdoor_humidity: entry_measurement(find_id(common, "0x07"))?,
        outdoor_temperature: entry_measurement(find_id(common, "0x02"))?,
        solar_radiation: entry_measurement(find_id(common, "0x15"))?,
        uv_index: entry_measurement(find_id(common, "0x17"))?,
        wet_bulb_globe_temperature: entry_measurement(find_id(common, "0xA2"))?,
        wind_direction: entry_measurement(find_id(common, "0x0A"))?,
        wind_gust: entry_measurement(find_id(common, "0x0C"))?,
        wind_speed: entry_measurement(find_id(common, "0x0B"))?,
    };
    let gateway = if let Some(entry) = gateway_group.first() {
        Some(EcowittGatewayReadings {
            indoor_humidity: field_measurement(Some(entry), "inhumi")?,
            indoor_temperature: field_measurement(Some(entry), "intemp")?,
            relative_pressure: field_measurement(Some(entry), "rel")?,
        })
    } else {
        None
    };
    Some(EcowittLivePayload {
        common: common_readings,
        gateway,
        group_count: object_array_count(root)?,
        particulate_sensors: particulate
            .iter()
            .map(decode_particulate)
            .collect::<Option<Vec<_>>>()?,
        piezo_rain: decode_rain(piezo_group)?,
        soil_sensors: soil.iter().map(decode_soil).collect::<Option<Vec<_>>>()?,
        temperature_sensors: temperature
            .iter()
            .map(decode_temperature)
            .collect::<Option<Vec<_>>>()?,
        traditional_rain: decode_rain(traditional_group)?,
    })
}

/// Decode one optional populated rain group.
fn decode_rain(group: &[JsonValue]) -> Option<Option<EcowittRainReadings>> {
    if group.is_empty() {
        return Some(None);
    }
    Some(Some(EcowittRainReadings {
        battery: battery(find_id(group, "0x13")),
        daily_total: entry_measurement(find_id(group, "0x10"))?,
        rate: entry_measurement(find_id(group, "0x0E"))?,
    }))
}

/// Decode one multi-channel temperature/humidity sensor.
fn decode_temperature(value: &JsonValue) -> Option<EcowittTemperatureSensor> {
    Some(EcowittTemperatureSensor {
        battery: battery(Some(value)),
        channel: channel(value)?,
        humidity: field_measurement(Some(value), "humidity")?,
        temperature: field_measurement(Some(value), "temp")?,
    })
}

/// Decode one multi-channel soil sensor.
fn decode_soil(value: &JsonValue) -> Option<EcowittSoilSensor> {
    Some(EcowittSoilSensor {
        battery: battery(Some(value)),
        channel: channel(value)?,
        conductivity: field_measurement(Some(value), "ec")?,
        moisture: field_measurement(Some(value), "humidity")?,
        temperature: field_measurement(Some(value), "temp")?,
    })
}

/// Decode one multi-channel particulate sensor.
fn decode_particulate(value: &JsonValue) -> Option<EcowittParticulateSensor> {
    Some(EcowittParticulateSensor {
        battery: battery(Some(value)),
        channel: channel(value)?,
        pm25: field_measurement(Some(value), "PM25")?,
    })
}

/// Decode one optional entry measurement.
fn entry_measurement(value: Option<&JsonValue>) -> Option<Option<EcowittMeasurement>> {
    field_measurement(value, "val")
}

/// Decode numeric fields, numeric strings, units, and documented no-data sentinels.
fn field_measurement(value: Option<&JsonValue>, key: &str) -> Option<Option<EcowittMeasurement>> {
    let Some(value) = value else {
        return Some(None);
    };
    let raw = field(value, key);
    // preserve missing and null provider cells
    let Some(raw) = raw else {
        return Some(None);
    };
    if matches!(raw, JsonValue::Null) {
        return Some(None);
    }
    let explicit_unit = text(value, "unit").unwrap_or_default().trim().to_owned();
    let (number, unit) = match raw {
        JsonValue::Number(number) if number.is_finite() => (*number, explicit_unit),
        JsonValue::String(units) => {
            let raw = String::from_utf16(units).ok()?;
            let raw = raw.trim();
            // preserve the documented case-insensitive no-data sentinels
            if no_data(raw) {
                return Some(None);
            }
            let (number, inline_unit) = numeric_prefix(raw)?;
            (
                number,
                if inline_unit.is_empty() {
                    explicit_unit
                } else {
                    inline_unit
                },
            )
        }
        _ => return None,
    };
    Some(Some(EcowittMeasurement {
        unit: unit_kind(&unit),
        value: number,
    }))
}

/// Recognize the gateway's textual missing-value sentinels.
fn no_data(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower == "none"
        || (value.len() >= 2 && value.bytes().all(|byte| byte == b'-'))
        || value == "--.-"
}

/// Split a finite decimal prefix from its optional inline unit.
fn numeric_prefix(value: &str) -> Option<(f64, String)> {
    let bytes = value.as_bytes();
    let mut index = usize::from(
        bytes
            .first()
            .is_some_and(|byte| matches!(*byte, b'+' | b'-')),
    );
    let start_digits = index;
    // consume whole digits before an optional decimal point
    while bytes.get(index).is_some_and(u8::is_ascii_digit) {
        index += 1;
    }
    let whole_digits = index > start_digits;
    let mut fractional_digits = false;
    let mut decimal_point = false;
    // consume the optional decimal point and following digits
    if bytes.get(index) == Some(&b'.') {
        decimal_point = true;
        index += 1;
        let fractional_start = index;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
        fractional_digits = index > fractional_start;
    }
    if (!whole_digits && !fractional_digits) || (decimal_point && !fractional_digits) {
        return None;
    }
    let number = value[..index].parse::<f64>().ok()?;
    number
        .is_finite()
        .then(|| (number, value[index..].trim().to_owned()))
}

/// Normalize one provider unit into the closed typed unit set.
fn unit_kind(value: &str) -> EcowittUnit {
    let normalized = value
        .trim()
        .to_lowercase()
        .replace('µ', "u")
        .replace('²', "2")
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>();
    match normalized.as_str() {
        "" => EcowittUnit::Unitless,
        "c" | "°c" => EcowittUnit::Celsius,
        "f" | "°f" => EcowittUnit::Fahrenheit,
        "hpa" => EcowittUnit::Hectopascal,
        "kpa" => EcowittUnit::Kilopascal,
        "inhg" => EcowittUnit::InchOfMercury,
        "in" => EcowittUnit::Inch,
        "in/hr" => EcowittUnit::InchPerHour,
        "km/h" => EcowittUnit::KilometerPerHour,
        "kn" | "knot" | "knots" => EcowittUnit::Knot,
        "m/s" => EcowittUnit::MeterPerSecond,
        "mm" => EcowittUnit::Millimeter,
        "mm/hr" => EcowittUnit::MillimeterPerHour,
        "%" => EcowittUnit::Percent,
        "ug/m3" => EcowittUnit::MicrogramsPerCubicMeter,
        "us/cm" => EcowittUnit::MicrosiemensPerCentimeter,
        "mph" => EcowittUnit::MilePerHour,
        "w/m2" => EcowittUnit::WattPerSquareMeter,
        _ => EcowittUnit::Unsupported,
    }
}

/// Decode a bounded channel from a number or one-to-two-digit string.
fn channel(value: &JsonValue) -> Option<u8> {
    let raw = field(value, "channel")?;
    let channel = match raw {
        JsonValue::Number(number)
            if number.is_finite()
                && number.fract() == 0.0
                && number.abs() <= MAXIMUM_SAFE_INTEGER =>
        {
            *number as i64
        }
        JsonValue::String(value) => {
            let value = String::from_utf16(value).ok()?;
            if value.is_empty()
                || value.len() > 2
                || !value.bytes().all(|byte| byte.is_ascii_digit())
            {
                return None;
            }
            value.parse().ok()?
        }
        _ => return None,
    };
    u8::try_from(channel)
        .ok()
        .filter(|channel| (1..=32).contains(channel))
}

/// Decode an optional battery value while retaining unrecognized provider states.
fn battery(value: Option<&JsonValue>) -> Option<EcowittBatteryState> {
    let value = value.and_then(|value| field(value, "battery"))?;
    let integer = match value {
        JsonValue::Number(number)
            if number.is_finite()
                && number.fract() == 0.0
                && number.abs() <= MAXIMUM_SAFE_INTEGER =>
        {
            Some(*number as i64)
        }
        JsonValue::String(value) => String::from_utf16(value)
            .ok()
            .filter(|value| value.len() == 1 && value.bytes().all(|byte| byte.is_ascii_digit()))
            .and_then(|value| value.parse().ok()),
        _ => None,
    };
    Some(integer.map_or(
        EcowittBatteryState::Unrecognized,
        EcowittBatteryState::Integer,
    ))
}

/// Find the first object with one exact provider ID.
fn find_id<'a>(values: &'a [JsonValue], id: &str) -> Option<&'a JsonValue> {
    values
        .iter()
        .find(|value| text(value, "id").as_deref() == Some(id))
}

/// Require an array field while distinguishing missing optional groups.
fn required_array<'a>(value: &'a JsonValue, key: &str) -> Option<&'a [JsonValue]> {
    let JsonValue::Array(values) = field(value, key)? else {
        return None;
    };
    Some(values)
}

/// Decode one absent-or-array optional group.
fn optional_array<'a>(value: &'a JsonValue, key: &str) -> Option<&'a [JsonValue]> {
    match field(value, key) {
        None => Some(&[]),
        Some(JsonValue::Array(values)) => Some(values),
        _ => None,
    }
}

/// Count every array-bearing top-level live-data group.
fn object_array_count(value: &JsonValue) -> Option<usize> {
    let JsonValue::Object(fields) = value else {
        return None;
    };
    Some(
        fields
            .iter()
            .filter(|(_, value)| matches!(value, JsonValue::Array(_)))
            .count(),
    )
}

/// Hash the exact two lowercase upstream digest spellings without a delimiter.
fn combined_checksum(identity: &Sha256Digest, live: &Sha256Digest) -> String {
    let mut preimage = String::with_capacity(128);
    preimage.push_str(identity.as_str());
    preimage.push_str(live.as_str());
    lower_hex(RevisionArchiveCanonicalCodec::sha256(
        &NativeWirePrimitives,
        preimage.as_bytes(),
    ))
}

/// Render a digest in lowercase hexadecimal.
fn lower_hex(digest: [u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(64);
    // emit exactly two lowercase nibbles per digest byte
    for byte in digest {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

/// Require one non-empty bounded JavaScript string.
fn bounded_text(value: &str, maximum: usize) -> bool {
    !value.trim().is_empty() && value.encode_utf16().count() <= maximum
}

/// Construct one stable Ecowitt payload failure.
fn ecowitt_failure() -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: "ecowitt_invalid_payload".to_owned(),
    }
}

/// Construct one provider-controller invalid payload failure.
fn invalid_attempt_failure(attempt: u8) -> ProviderAttemptError {
    ProviderAttemptError::Classified(weather_providers::ProviderFailure::classified(
        attempt,
        weather_providers::ProviderFailureClassification::InvalidPayload,
        "ecowitt_invalid_payload",
        "Ecowitt gateway payload is invalid",
        Some(200),
    ))
}

/// Construct one provider-controller shared-state transport failure.
fn transport_attempt_failure(attempt: u8) -> ProviderAttemptError {
    ProviderAttemptError::Transport {
        message: format!("Ecowitt request state is unavailable at attempt {attempt}"),
    }
}

#[cfg(test)]
#[path = "native_ecowitt_transport_test.rs"]
mod tests;
