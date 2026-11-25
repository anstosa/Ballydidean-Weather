//! Native public-station and tide catalog trust boundaries.

use std::{fs::File, io::Read, path::Path};

use url::Url;
use weather_contracts::{
    JsonValue, Nullable, PublicStationAdapter, PublicStationConfiguration,
    PublicStationConfigurationStation, PublicStationProviderConfiguration,
    PublicStationSourceConfiguration, SourceCapability, StationKind, TideConfiguration,
    TideProviderConfiguration, TideSourceConfiguration, TideSourceConfigurationSourceKindEnum,
    TideStationConfiguration,
};
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};

use crate::{
    NativeWirePrimitives,
    api_shadow_legacy_values::js_trim_unit,
    maintenance_canonical_json::{exact_keys, field},
};

const MAXIMUM_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

/// Parse and validate the checked public-station catalog.
pub(crate) fn parse_public_station_configuration(
    bytes: &[u8],
) -> Option<PublicStationConfiguration> {
    let primitives = NativeWirePrimitives::new();
    let root = primitives.decode_json(bytes)?;
    require_object(&root)?;
    (require_number(field(&root, "version")?)? == 1.0).then_some(())?;
    let site_key = require_stable_key(field(&root, "siteKey")?, 80)?;
    let provider_values = require_array(field(&root, "providers")?)?;
    let mut providers = Vec::with_capacity(provider_values.len());
    // preserve provider declaration order
    for provider in provider_values {
        providers.push(parse_public_provider(provider)?);
    }
    // reject ambiguous provider identities
    if has_duplicate_keys(providers.iter().map(|provider| provider.key.as_str())) {
        return None;
    }
    let station_values = require_array(field(&root, "stations")?)?;
    let mut stations = Vec::with_capacity(station_values.len());
    // preserve physical-station declaration order
    for station in station_values {
        stations.push(parse_public_station(
            station,
            &site_key,
            &providers,
            &primitives,
        )?);
    }
    // reject ambiguous station identities
    if has_duplicate_keys(stations.iter().map(|station| station.key.as_str())) {
        return None;
    }
    // reject source identities duplicated across different stations
    if has_duplicate_keys(
        stations
            .iter()
            .flat_map(|station| station.sources.iter().map(|source| source.key.as_str())),
    ) {
        return None;
    }
    Some(PublicStationConfiguration {
        siteKey: site_key,
        providers,
        stations,
        version: 1,
    })
}

/// Parse and validate the checked NOAA tide catalog.
pub(crate) fn parse_tide_configuration(bytes: &[u8]) -> Option<TideConfiguration> {
    let primitives = NativeWirePrimitives::new();
    let root = primitives.decode_json(bytes)?;
    require_object(&root)?;
    (require_number(field(&root, "version")?)? == 1.0).then_some(())?;
    let provider = parse_tide_provider(field(&root, "provider")?)?;
    let site_key = require_stable_key(field(&root, "siteKey")?, 80)?;
    let station_values = require_nonempty_array(field(&root, "stations")?)?;
    let mut stations = Vec::with_capacity(station_values.len());
    // preserve station declaration order
    for station in station_values {
        stations.push(parse_tide_station(
            station,
            &provider.key,
            &site_key,
            &primitives,
        )?);
    }
    let observation_count = stations
        .iter()
        .filter(|station| {
            station.source.sourceKind == TideSourceConfigurationSourceKindEnum::TideObservation
        })
        .count();
    let prediction_count = stations
        .iter()
        .filter(|station| {
            station.source.sourceKind == TideSourceConfigurationSourceKindEnum::TidePrediction
        })
        .count();
    // require exactly one observation and one prediction contract
    if stations.len() != 2 || observation_count != 1 || prediction_count != 1 {
        return None;
    }
    // reject duplicate durable station or source identities
    if has_duplicate_keys(stations.iter().map(|station| station.key.as_str()))
        || has_duplicate_keys(stations.iter().map(|station| station.source.key.as_str()))
    {
        return None;
    }
    Some(TideConfiguration {
        provider,
        siteKey: site_key,
        stations,
        version: 1,
    })
}

/// Parse one public-station provider definition.
fn parse_public_provider(value: &JsonValue) -> Option<PublicStationProviderConfiguration> {
    require_object(value)?;
    Some(PublicStationProviderConfiguration {
        active: require_boolean(field(value, "active")?)?,
        attributionLabel: require_string(field(value, "attributionLabel")?, 256)?,
        attributionUrl: require_http_url(field(value, "attributionUrl")?, false)?,
        displayName: require_string(field(value, "displayName")?, 160)?,
        key: require_stable_key(field(value, "key")?, 80)?,
    })
}

/// Parse one physical public station and all of its immutable sources.
fn parse_public_station(
    value: &JsonValue,
    site_key: &str,
    providers: &[PublicStationProviderConfiguration],
    primitives: &NativeWirePrimitives,
) -> Option<PublicStationConfigurationStation> {
    require_object(value)?;
    let key = require_stable_key(field(value, "key")?, 80)?;
    let latitude = require_number(field(value, "latitude")?)?;
    let longitude = require_number(field(value, "longitude")?)?;
    require_coordinates(latitude, longitude)?;
    let timezone = require_timezone(field(value, "timezone")?)?;
    let source_values = require_array(field(value, "sources")?)?;
    let mut sources = Vec::with_capacity(source_values.len());
    // preserve source declaration order
    for source in source_values {
        sources.push(parse_public_source(
            source, site_key, &key, latitude, longitude, &timezone, providers, primitives,
        )?);
    }
    // require at least one usable source
    if sources.is_empty() {
        return None;
    }
    Some(PublicStationConfigurationStation {
        active: require_boolean(field(value, "active")?)?,
        displayName: require_string(field(value, "displayName")?, 160)?,
        key,
        latitude,
        longitude,
        model: optional_string(field(value, "model")?, 128)?,
        serial: optional_string(field(value, "serial")?, 128)?,
        sources,
        timezone,
        vendor: optional_string(field(value, "vendor")?, 128)?,
    })
}

/// Parse one immutable public-station provider source.
#[allow(clippy::too_many_arguments)]
fn parse_public_source(
    value: &JsonValue,
    site_key: &str,
    station_key: &str,
    latitude: f64,
    longitude: f64,
    timezone: &str,
    providers: &[PublicStationProviderConfiguration],
    primitives: &NativeWirePrimitives,
) -> Option<PublicStationSourceConfiguration> {
    require_object(value)?;
    let key = require_stable_key(field(value, "key")?, 80)?;
    let provider_key = require_stable_key(field(value, "providerKey")?, 80)?;
    providers
        .iter()
        .any(|provider| provider.key == provider_key)
        .then_some(())?;
    let adapter = require_public_adapter(field(value, "adapter")?)?;
    let capabilities = require_capabilities(field(value, "capabilities")?)?;
    let adapter_config = require_public_adapter_config(field(value, "adapterConfig")?, adapter)?;
    let history_start_date = optional_date(field(value, "historyStartDate")?)?;
    let history_end_date = optional_date(field(value, "historyEndDate")?)?;
    // bind historical sources to a concrete available range
    if capabilities.contains(&SourceCapability::Historical) && history_start_date == Nullable::Null
    {
        return None;
    }
    // reject reversed archive bounds
    if let (Nullable::Value(start), Nullable::Value(end)) = (&history_start_date, &history_end_date)
        && start > end
    {
        return None;
    }
    let cadence_seconds = optional_integer(field(value, "cadenceSeconds")?, 60, 86_400)?;
    // require scheduled current sources to have a cadence
    if capabilities.contains(&SourceCapability::Current)
        != matches!(cadence_seconds, Nullable::Value(_))
    {
        return None;
    }
    let fingerprint = source_fingerprint(
        primitives,
        JsonValue::Object(adapter_config.clone()),
        latitude,
        longitude,
        site_key,
        timezone,
        &provider_key,
        &key,
        "physical_sensor",
        station_key,
    )?;
    Some(PublicStationSourceConfiguration {
        active: require_boolean(field(value, "active")?)?,
        adapter,
        adapterConfig: adapter_config,
        cadenceSeconds: cadence_seconds,
        capabilities,
        fingerprint,
        historyEndDate: history_end_date,
        historyStartDate: history_start_date,
        key,
        maximumChunkDays: require_integer(field(value, "maximumChunkDays")?, 1, 31)?,
        providerKey: provider_key,
    })
}

/// Parse one tide provider definition.
fn parse_tide_provider(value: &JsonValue) -> Option<TideProviderConfiguration> {
    require_object(value)?;
    Some(TideProviderConfiguration {
        active: require_boolean(field(value, "active")?)?,
        attributionLabel: require_string(field(value, "attributionLabel")?, 256)?,
        attributionUrl: require_http_url(field(value, "attributionUrl")?, true)?,
        displayName: require_string(field(value, "displayName")?, 160)?,
        key: require_stable_key(field(value, "key")?, 80)?,
    })
}

/// Parse one NOAA tide station and synthesize its immutable adapter contract.
fn parse_tide_station(
    value: &JsonValue,
    provider_key: &str,
    site_key: &str,
    primitives: &NativeWirePrimitives,
) -> Option<TideStationConfiguration> {
    require_object(value)?;
    let key = require_stable_key(field(value, "key")?, 80)?;
    let latitude = require_number(field(value, "latitude")?)?;
    let longitude = require_number(field(value, "longitude")?)?;
    require_coordinates(latitude, longitude)?;
    let timezone = require_timezone(field(value, "timezone")?)?;
    let source_value = field(value, "source")?;
    require_object(source_value)?;
    let source_kind = require_tide_source_kind(field(source_value, "sourceKind")?)?;
    let source_key = require_stable_key(field(source_value, "key")?, 80)?;
    let station_id = require_station_id(field(value, "serial")?)?;
    let (adapter_config, capabilities, maximum_chunk_days) = match source_kind {
        // construct the observation contract in its original insertion order
        TideSourceConfigurationSourceKindEnum::TideObservation => (
            vec![
                member("contractVersion", string("noaa-water-level/v1")),
                member("datum", string("MLLW")),
                member("product", string("water_level")),
                member("sample", string("six-minute-verified-water-level")),
                member("stationId", string(&station_id)),
            ],
            vec![SourceCapability::Current, SourceCapability::Historical],
            require_integer(field(source_value, "maximumChunkDays")?, 1, 31)?,
        ),
        // construct the prediction contract in its original insertion order
        TideSourceConfigurationSourceKindEnum::TidePrediction => (
            vec![
                member("contractVersion", string("noaa-tide-predictions/v1")),
                member("datum", string("MLLW")),
                member("forecastDays", JsonValue::Number(30.0)),
                member("interval", string("hilo")),
                member("product", string("predictions")),
                member("sample", string("high-low-events")),
                member("stationId", string(&station_id)),
            ],
            vec![SourceCapability::Forecast, SourceCapability::Historical],
            require_integer(field(source_value, "maximumChunkDays")?, 1, 366)?,
        ),
    };
    let source_kind_text = match source_kind {
        TideSourceConfigurationSourceKindEnum::TideObservation => "tide_observation",
        TideSourceConfigurationSourceKindEnum::TidePrediction => "tide_prediction",
    };
    let fingerprint = source_fingerprint(
        primitives,
        JsonValue::Object(adapter_config.clone()),
        latitude,
        longitude,
        site_key,
        &timezone,
        provider_key,
        &source_key,
        source_kind_text,
        &key,
    )?;
    Some(TideStationConfiguration {
        active: require_boolean(field(value, "active")?)?,
        displayName: require_string(field(value, "displayName")?, 160)?,
        key,
        kind: require_station_kind(field(value, "kind")?)?,
        latitude,
        longitude,
        model: require_string(field(value, "model")?, 128)?,
        serial: station_id,
        source: TideSourceConfiguration {
            active: require_boolean(field(source_value, "active")?)?,
            adapterConfig: adapter_config,
            cadenceSeconds: require_integer(field(source_value, "cadenceSeconds")?, 360, 86_400)?,
            capabilities,
            fingerprint,
            historyStartDate: require_date(field(source_value, "historyStartDate")?)?,
            key: source_key,
            maximumChunkDays: maximum_chunk_days,
            sourceKind: source_kind,
        },
        timezone,
        vendor: require_string(field(value, "vendor")?, 128)?,
    })
}

/// Validate and retain one adapter-specific immutable JSON object.
fn require_public_adapter_config(
    value: &JsonValue,
    adapter: PublicStationAdapter,
) -> Option<Vec<(Vec<u16>, JsonValue)>> {
    let fields = require_object(value)?;
    // apply the closed contract selected by the adapter
    match adapter {
        PublicStationAdapter::AmbientWeather => {
            exact_keys(
                value,
                &["contractVersion", "deviceId", "macAddress", "sample"],
            )
            .then_some(())?;
            require_literal(field(value, "contractVersion")?, "ambient-device-data/v1")?;
            require_lower_hex(field(value, "deviceId")?, 24)?;
            require_mac_address(field(value, "macAddress")?, true)?;
            require_literal(
                field(value, "sample")?,
                "every-distinct-provider-observation",
            )?;
        }
        PublicStationAdapter::WeatherUnderground => {
            exact_keys(
                value,
                &[
                    "contractVersion",
                    "deduplicatesSourceKey",
                    "publicApiKey",
                    "sample",
                    "stationId",
                ],
            )
            .then_some(())?;
            require_literal(
                field(value, "contractVersion")?,
                "wunderground-pws-history/v1",
            )?;
            require_ascii_identifier(field(value, "stationId")?, 3, 32)?;
            require_lower_hex(field(value, "publicApiKey")?, 32)?;
            require_literal(field(value, "sample")?, "five-minute-provider-observations")?;
            require_stable_key(field(value, "deduplicatesSourceKey")?, 80)?;
        }
        PublicStationAdapter::Purpleair => {
            exact_keys(
                value,
                &["contractVersion", "mapVersion", "sample", "sensorIndex"],
            )
            .then_some(())?;
            require_literal(field(value, "contractVersion")?, "purpleair-map-history/v1")?;
            require_semantic_version(field(value, "mapVersion")?)?;
            require_integer(field(value, "sensorIndex")?, 1, 10_000_000)?;
            require_literal(
                field(value, "sample")?,
                "every-two-minute-dual-channel-average",
            )?;
        }
        PublicStationAdapter::Netatmo => {
            exact_keys(
                value,
                &[
                    "contractVersion",
                    "deviceId",
                    "outdoorModuleId",
                    "rainModuleId",
                    "sample",
                    "windModuleId",
                ],
            )
            .then_some(())?;
            require_literal(
                field(value, "contractVersion")?,
                "netatmo-public-measures/v1",
            )?;
            require_mac_address(field(value, "deviceId")?, false)?;
            require_mac_address(field(value, "outdoorModuleId")?, false)?;
            require_mac_address(field(value, "rainModuleId")?, false)?;
            require_mac_address(field(value, "windModuleId")?, false)?;
            require_literal(field(value, "sample")?, "five-minute-provider-buckets")?;
        }
    }
    Some(fields.clone())
}

/// Hash one source's recursively sorted canonical material without archive framing.
#[allow(clippy::too_many_arguments)]
fn source_fingerprint(
    primitives: &NativeWirePrimitives,
    adapter_config: JsonValue,
    latitude: f64,
    longitude: f64,
    site_key: &str,
    timezone: &str,
    provider_key: &str,
    source_key: &str,
    source_kind: &str,
    station_key: &str,
) -> Option<String> {
    let material = JsonValue::Object(vec![
        member("adapterConfig", adapter_config),
        member(
            "location",
            JsonValue::Object(vec![
                member("latitude", JsonValue::Number(latitude)),
                member("longitude", JsonValue::Number(longitude)),
                member("siteKey", string(site_key)),
                member("timezone", string(timezone)),
            ]),
        ),
        member("providerKey", string(provider_key)),
        member("sourceKey", string(source_key)),
        member("sourceKind", string(source_kind)),
        member("stationKey", string(station_key)),
        member("version", JsonValue::Number(1.0)),
    ]);
    let mut canonical = primitives.encode_sorted_json(&material)?;
    // remove the archive framing absent from canonicalizeJson
    (canonical.pop() == Some(b'\n')).then_some(())?;
    let digest = RevisionArchiveCanonicalCodec::sha256(primitives, &canonical);
    Some(lower_hex(digest))
}

/// Require one JSON object and return its retained property order.
fn require_object(value: &JsonValue) -> Option<&Vec<(Vec<u16>, JsonValue)>> {
    // reject nulls, arrays, and primitives
    match value {
        JsonValue::Object(fields) => Some(fields),
        _ => None,
    }
}

/// Require one JSON array while preserving declaration order.
fn require_array(value: &JsonValue) -> Option<&[JsonValue]> {
    // reject non-array values
    match value {
        JsonValue::Array(values) => Some(values),
        _ => None,
    }
}

/// Require one non-empty JSON array.
fn require_nonempty_array(value: &JsonValue) -> Option<&[JsonValue]> {
    let values = require_array(value)?;
    (!values.is_empty()).then_some(values)
}

/// Require one bounded, non-whitespace JSON string.
fn require_string(value: &JsonValue, maximum: usize) -> Option<String> {
    let JsonValue::String(units) = value else {
        return None;
    };
    let text = String::from_utf16(units).ok()?;
    (units.iter().any(|unit| !js_trim_unit(*unit)) && units.len() <= maximum).then_some(text)
}

/// Parse one nullable bounded string without inventing a missing default.
fn optional_string(value: &JsonValue, maximum: usize) -> Option<Nullable<String>> {
    // preserve explicit JSON null independently from a string
    match value {
        JsonValue::Null => Some(Nullable::Null),
        _ => require_string(value, maximum).map(Nullable::Value),
    }
}

/// Require one finite JSON number.
fn require_number(value: &JsonValue) -> Option<f64> {
    let JsonValue::Number(number) = value else {
        return None;
    };
    number.is_finite().then_some(*number)
}

/// Require one JavaScript-safe integer inside inclusive unsigned bounds.
fn require_integer(value: &JsonValue, minimum: u64, maximum: u64) -> Option<u64> {
    let number = require_number(value)?;
    (number.fract() == 0.0
        && number.abs() <= MAXIMUM_SAFE_INTEGER
        && number >= minimum as f64
        && number <= maximum as f64)
        .then_some(number as u64)
}

/// Parse one nullable safe integer inside inclusive bounds.
fn optional_integer(value: &JsonValue, minimum: u64, maximum: u64) -> Option<Nullable<u64>> {
    // preserve explicit JSON null independently from a number
    match value {
        JsonValue::Null => Some(Nullable::Null),
        _ => require_integer(value, minimum, maximum).map(Nullable::Value),
    }
}

/// Require one exact JSON boolean without truthy coercion.
fn require_boolean(value: &JsonValue) -> Option<bool> {
    // reject non-boolean values
    match value {
        JsonValue::Boolean(boolean) => Some(*boolean),
        _ => None,
    }
}

/// Require one portable lowercase kebab-case identity.
fn require_stable_key(value: &JsonValue, maximum: usize) -> Option<String> {
    let text = require_string(value, maximum)?;
    let bytes = text.as_bytes();
    let valid = !bytes.is_empty()
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes[bytes.len() - 1].is_ascii_alphanumeric()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
        && !bytes.windows(2).any(|pair| pair == b"--");
    valid.then_some(text)
}

/// Require finite latitude and longitude inside their geographic bounds.
fn require_coordinates(latitude: f64, longitude: f64) -> Option<()> {
    (latitude.is_finite()
        && (-90.0..=90.0).contains(&latitude)
        && longitude.is_finite()
        && (-180.0..=180.0).contains(&longitude))
    .then_some(())
}

/// Require an IANA timezone backed by the host's system timezone database.
fn require_timezone(value: &JsonValue) -> Option<String> {
    let timezone = require_string(value, 64)?;
    let valid_name = timezone.split('/').all(|component| {
        !component.is_empty()
            && component != "."
            && component != ".."
            && component.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'+' | b'.')
            })
    });
    valid_name.then_some(())?;
    let path = Path::new("/usr/share/zoneinfo").join(&timezone);
    let mut file = File::open(path).ok()?;
    let mut magic = [0_u8; 4];
    file.read_exact(&mut magic).ok()?;
    (magic == *b"TZif").then_some(timezone)
}

/// Require an absolute HTTP URL and optionally reject embedded credentials.
fn require_http_url(value: &JsonValue, public: bool) -> Option<String> {
    let text = require_string(value, 512)?;
    let url = Url::parse(&text).ok()?;
    matches!(url.scheme(), "http" | "https").then_some(())?;
    // reject non-empty credentials at the tide public boundary
    if public
        && (!url.username().is_empty() || url.password().is_some_and(|value| !value.is_empty()))
    {
        return None;
    }
    Some(url.to_string())
}

/// Require one exact string literal.
fn require_literal(value: &JsonValue, expected: &str) -> Option<()> {
    let JsonValue::String(units) = value else {
        return None;
    };
    units
        .iter()
        .copied()
        .eq(expected.encode_utf16())
        .then_some(())
}

/// Require one supported public-station adapter.
fn require_public_adapter(value: &JsonValue) -> Option<PublicStationAdapter> {
    let value = require_string(value, usize::MAX)?;
    // map only the original closed adapter set
    match value.as_str() {
        "ambient-weather" => Some(PublicStationAdapter::AmbientWeather),
        "weather-underground" => Some(PublicStationAdapter::WeatherUnderground),
        "netatmo" => Some(PublicStationAdapter::Netatmo),
        "purpleair" => Some(PublicStationAdapter::Purpleair),
        _ => None,
    }
}

/// Require one non-empty, unique, supported capability list.
fn require_capabilities(value: &JsonValue) -> Option<Vec<SourceCapability>> {
    let values = require_array(value)?;
    (!values.is_empty()).then_some(())?;
    let mut capabilities = Vec::with_capacity(values.len());
    // preserve source capability order while closing the supported set
    for value in values {
        let text = require_string(value, usize::MAX)?;
        let capability = match text.as_str() {
            "current" => SourceCapability::Current,
            "historical" => SourceCapability::Historical,
            "forecast" => SourceCapability::Forecast,
            "stream" => SourceCapability::Stream,
            _ => return None,
        };
        // reject duplicate semantic capabilities
        if capabilities.contains(&capability) {
            return None;
        }
        capabilities.push(capability);
    }
    Some(capabilities)
}

/// Require one supported tide source kind.
fn require_tide_source_kind(value: &JsonValue) -> Option<TideSourceConfigurationSourceKindEnum> {
    let value = require_string(value, usize::MAX)?;
    // map only tide semantics
    match value.as_str() {
        "tide_observation" => Some(TideSourceConfigurationSourceKindEnum::TideObservation),
        "tide_prediction" => Some(TideSourceConfigurationSourceKindEnum::TidePrediction),
        _ => None,
    }
}

/// Require one physical or virtual station kind.
fn require_station_kind(value: &JsonValue) -> Option<StationKind> {
    let value = require_string(value, usize::MAX)?;
    // map only the original station set
    match value.as_str() {
        "physical" => Some(StationKind::Physical),
        "virtual" => Some(StationKind::Virtual),
        _ => None,
    }
}

/// Require a seven-digit NOAA station identifier.
fn require_station_id(value: &JsonValue) -> Option<String> {
    let station_id = require_string(value, 7)?;
    (station_id.len() == 7 && station_id.bytes().all(|byte| byte.is_ascii_digit()))
        .then_some(station_id)
}

/// Parse one optional strict Gregorian calendar date.
fn optional_date(value: &JsonValue) -> Option<Nullable<String>> {
    // preserve explicit JSON null independently from a calendar date
    match value {
        JsonValue::Null => Some(Nullable::Null),
        _ => require_date(value).map(Nullable::Value),
    }
}

/// Require one strict Gregorian YYYY-MM-DD calendar date.
fn require_date(value: &JsonValue) -> Option<String> {
    let date = require_string(value, 10)?;
    let bytes = date.as_bytes();
    let spelling = bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit());
    spelling.then_some(())?;
    let parsed = date.parse::<jiff::civil::Date>().ok()?;
    (parsed.to_string() == date).then_some(date)
}

/// Require an exact-length lowercase hexadecimal string.
fn require_lower_hex(value: &JsonValue, length: usize) -> Option<()> {
    let value = require_string(value, length)?;
    (value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)))
    .then_some(())
}

/// Require an uppercase or lowercase six-octet colon-separated MAC address.
fn require_mac_address(value: &JsonValue, uppercase: bool) -> Option<()> {
    let value = require_string(value, 17)?;
    let bytes = value.as_bytes();
    let valid = bytes.len() == 17
        && bytes.iter().enumerate().all(|(index, byte)| {
            // delimit every hexadecimal octet with a colon
            if matches!(index, 2 | 5 | 8 | 11 | 14) {
                *byte == b':'
            } else if uppercase {
                byte.is_ascii_digit() || (b'A'..=b'F').contains(byte)
            } else {
                byte.is_ascii_digit() || (b'a'..=b'f').contains(byte)
            }
        });
    valid.then_some(())
}

/// Require an uppercase alphanumeric provider station identifier.
fn require_ascii_identifier(value: &JsonValue, minimum: usize, maximum: usize) -> Option<()> {
    let value = require_string(value, maximum)?;
    (value.len() >= minimum
        && value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit()))
    .then_some(())
}

/// Require a three-component numeric semantic version spelling.
fn require_semantic_version(value: &JsonValue) -> Option<()> {
    let value = require_string(value, usize::MAX)?;
    let mut components = value.split('.');
    // require exactly three non-empty digit components
    for _ in 0..3 {
        let component = components.next()?;
        (!component.is_empty() && component.bytes().all(|byte| byte.is_ascii_digit()))
            .then_some(())?;
    }
    components.next().is_none().then_some(())
}

/// Report whether a sequence contains a repeated string identity.
fn has_duplicate_keys<'a>(values: impl IntoIterator<Item = &'a str>) -> bool {
    let mut retained = Vec::<&str>::new();
    // catalogs are small, so avoid allocating a hash table for uniqueness checks
    for value in values {
        // stop at the first repeated durable identity
        if retained.contains(&value) {
            return true;
        }
        retained.push(value);
    }
    false
}

/// Build one UTF-16 JSON object member.
fn member(key: &str, value: JsonValue) -> (Vec<u16>, JsonValue) {
    (key.encode_utf16().collect(), value)
}

/// Build one generated JSON string.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Render a SHA-256 digest using Node's lowercase hexadecimal spelling.
fn lower_hex(digest: [u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut rendered = String::with_capacity(64);
    // emit exactly two lowercase digits for each digest byte
    for byte in digest {
        rendered.push(char::from(HEX[usize::from(byte >> 4)]));
        rendered.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    rendered
}
