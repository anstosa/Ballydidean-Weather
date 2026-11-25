//! Native parsing for the worker's site, Ecowitt, and Tempest catalogs.

use std::collections::HashSet;
use std::path::PathBuf;

use jiff::civil::Date;
use url::Url;
use weather_contracts::{
    EcowittConfiguration, EcowittConfigurationProvider, EcowittStationConfiguration, JsonValue,
    Nullable, SiteConfiguration, SiteConfigurationProvider, SiteConfigurationSite,
    SiteConfigurationSource, SiteConfigurationStation, SourceCapability, SourceKind, StationKind,
    TempestConfiguration, TempestConfigurationProvider, TempestStationConfiguration,
};
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};

use crate::NativeWirePrimitives;
use crate::maintenance_canonical_json::field;

const MAXIMUM_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;
const ECOWITT_SOURCE_CONTRACT_VERSION: &str = "ecowitt-local-live/v1";
const TEMPEST_SOURCE_CONTRACT_VERSION: &str = "tempest-observations/v2";

/// Parse one site catalog through the approved native JSON boundary.
pub(crate) fn parse_site_configuration(bytes: &[u8]) -> Option<SiteConfiguration> {
    let root = decode_object(bytes)?;
    let version = exact_version(field(&root, "version")?)?;
    let site = object(field(&root, "site")?)?;
    let station = object(field(&root, "station")?)?;
    let provider = object(field(&root, "provider")?)?;
    let latitude = finite_number(field(site, "latitude")?)?;
    let longitude = finite_number(field(site, "longitude")?)?;
    validate_coordinates(latitude, longitude)?;
    let site_key = stable_key(field(site, "key")?)?;
    let timezone = time_zone(field(site, "timezone")?)?;
    let station_kind = station_kind(field(station, "kind")?)?;
    let provider_key = stable_key(field(provider, "key")?)?;
    let station_key = stable_key(field(station, "key")?)?;
    let source_values = array(field(&root, "sources")?)?;

    // require at least one source as the TypeScript boundary does
    if source_values.is_empty() {
        return None;
    }
    let mut sources = Vec::with_capacity(source_values.len());
    // preserve source order while deriving each immutable fingerprint
    for source in source_values {
        sources.push(parse_site_source(
            source,
            version,
            &provider_key,
            &station_key,
            latitude,
            longitude,
            &site_key,
            &timezone,
        )?);
    }
    // reject duplicate durable source identities
    if !unique_strings(sources.iter().map(|source| source.key.as_str())) {
        return None;
    }

    Some(SiteConfiguration {
        provider: SiteConfigurationProvider {
            active: boolean(field(provider, "active")?)?,
            attributionLabel: nonempty_bounded_string(field(provider, "attributionLabel")?, 256)?,
            attributionUrl: http_url(field(provider, "attributionUrl")?, None, true)?,
            displayName: nonempty_bounded_string(field(provider, "displayName")?, 160)?,
            key: provider_key,
        },
        site: SiteConfigurationSite {
            active: boolean(field(site, "active")?)?,
            displayName: nonempty_bounded_string(field(site, "displayName")?, 160)?,
            key: site_key,
            latitude,
            longitude,
            timezone,
        },
        sources,
        station: SiteConfigurationStation {
            active: boolean(field(station, "active")?)?,
            displayName: nonempty_bounded_string(field(station, "displayName")?, 160)?,
            key: station_key,
            kind: station_kind,
            latitude,
            longitude,
            model: optional_bounded_string(field(station, "model"), 128)?,
            serial: optional_bounded_string(field(station, "serial"), 128)?,
            vendor: optional_bounded_string(field(station, "vendor"), 128)?,
        },
        version,
    })
}

/// Parse one checked Ecowitt gateway catalog.
pub(crate) fn parse_ecowitt_configuration(bytes: &[u8]) -> Option<EcowittConfiguration> {
    let root = decode_object(bytes)?;
    let version = exact_version(field(&root, "version")?)?;
    let provider = object(field(&root, "provider")?)?;
    let provider_key = stable_key(field(provider, "key")?)?;
    let site_key = stable_key(field(&root, "siteKey")?)?;
    let station_values = array(field(&root, "stations")?)?;

    // require at least one local gateway
    if station_values.is_empty() {
        return None;
    }
    let mut stations = Vec::with_capacity(station_values.len());
    // retain configured gateway order
    for station in station_values {
        stations.push(parse_ecowitt_station(
            station,
            &provider_key,
            &site_key,
            version,
        )?);
    }
    // preserve one-to-one identity across every gateway field
    if !unique_strings(stations.iter().map(|station| station.key.as_str()))
        || !unique_strings(stations.iter().map(|station| station.sourceKey.as_str()))
        || !unique_strings(stations.iter().map(|station| station.expectedMac.as_str()))
        || !unique_strings(stations.iter().map(|station| station.gatewayHost.as_str()))
    {
        return None;
    }

    Some(EcowittConfiguration {
        provider: EcowittConfigurationProvider {
            active: boolean(field(provider, "active")?)?,
            attributionLabel: nonempty_bounded_string(field(provider, "attributionLabel")?, 256)?,
            attributionUrl: http_url(field(provider, "attributionUrl")?, Some(512), false)?,
            displayName: nonempty_bounded_string(field(provider, "displayName")?, 160)?,
            key: provider_key,
        },
        siteKey: site_key,
        stations,
        version,
    })
}

/// Parse one checked Tempest station catalog.
pub(crate) fn parse_tempest_configuration(bytes: &[u8]) -> Option<TempestConfiguration> {
    let root = decode_object(bytes)?;
    let version = exact_version(field(&root, "version")?)?;
    let provider = object(field(&root, "provider")?)?;
    let provider_key = stable_key(field(provider, "key")?)?;
    let site_key = stable_key(field(&root, "siteKey")?)?;
    let station_values = array(field(&root, "stations")?)?;

    // require at least one configured station
    if station_values.is_empty() {
        return None;
    }
    let mut stations = Vec::with_capacity(station_values.len());
    // retain configured station order
    for station in station_values {
        stations.push(parse_tempest_station(
            station,
            &provider_key,
            &site_key,
            version,
        )?);
    }
    // preserve one-to-one identity across every Tempest field
    if !unique_strings(stations.iter().map(|station| station.key.as_str()))
        || !unique_strings(stations.iter().map(|station| station.sourceKey.as_str()))
        || !unique_u64(stations.iter().map(|station| station.locationId))
        || !unique_u64(stations.iter().map(|station| station.deviceId))
    {
        return None;
    }

    Some(TempestConfiguration {
        provider: TempestConfigurationProvider {
            active: boolean(field(provider, "active")?)?,
            attributionLabel: nonempty_bounded_string(field(provider, "attributionLabel")?, 256)?,
            attributionUrl: http_url(field(provider, "attributionUrl")?, Some(512), false)?,
            displayName: nonempty_bounded_string(field(provider, "displayName")?, 160)?,
            key: provider_key,
        },
        siteKey: site_key,
        stations,
        version,
    })
}

/// Parse and fingerprint one source from the site catalog.
#[allow(clippy::too_many_arguments)]
fn parse_site_source(
    value: &JsonValue,
    version: u64,
    provider_key: &str,
    station_key: &str,
    latitude: f64,
    longitude: f64,
    site_key: &str,
    timezone: &str,
) -> Option<SiteConfigurationSource> {
    let source = object(value)?;
    let key = stable_key(field(source, "key")?)?;
    let source_kind = source_kind(field(source, "sourceKind")?)?;
    let adapter_config = bounded_adapter_object(field(source, "adapterConfig")?)?;
    let capability_values = array(field(source, "capabilities")?)?;

    // require a non-empty closed capability list
    if capability_values.is_empty() {
        return None;
    }
    let mut capabilities = Vec::with_capacity(capability_values.len());
    // deduplicate capabilities in first-seen order like a JavaScript Set
    for value in capability_values {
        let capability = source_capability(value)?;
        // retain only the first occurrence
        if !capabilities.contains(&capability) {
            capabilities.push(capability);
        }
    }
    let cadence_seconds = match field(source, "cadenceSeconds")? {
        // preserve explicit null cadence
        JsonValue::Null => Nullable::Null,
        // admit only safe bounded integer cadence
        value => Nullable::Value(bounded_integer(value, 60, 31_536_000)?),
    };
    let fingerprint = source_fingerprint(
        &adapter_config,
        latitude,
        longitude,
        site_key,
        timezone,
        provider_key,
        &key,
        source_kind,
        station_key,
        version,
    )?;

    Some(SiteConfigurationSource {
        active: boolean(field(source, "active")?)?,
        adapterConfig: adapter_config,
        cadenceSeconds: cadence_seconds,
        capabilities,
        fingerprint,
        key,
        sourceKind: source_kind,
    })
}

/// Parse and derive one Ecowitt gateway identity.
fn parse_ecowitt_station(
    value: &JsonValue,
    provider_key: &str,
    site_key: &str,
    version: u64,
) -> Option<EcowittStationConfiguration> {
    let station = object(value)?;
    let key = stable_key(field(station, "key")?)?;
    let latitude = finite_number(field(station, "latitude")?)?;
    let longitude = finite_number(field(station, "longitude")?)?;
    validate_coordinates(latitude, longitude)?;
    let timezone = time_zone(field(station, "timezone")?)?;
    let gateway_host = private_ipv4(field(station, "gatewayHost")?)?;
    let expected_mac = canonical_mac(field(station, "expectedMac")?)?;
    let compact_mac = expected_mac.replace(':', "").to_ascii_lowercase();
    let source_key = format!("ecowitt-{compact_mac}-local-live-v1");
    let adapter_config = object_value([
        (
            "contractVersion",
            string_value(ECOWITT_SOURCE_CONTRACT_VERSION),
        ),
        ("endpointPath", string_value("/get_livedata_info")),
        ("expectedMac", string_value(&expected_mac)),
        ("gatewayHost", string_value(&gateway_host)),
        ("measurementSet", string_value("canonical-primary")),
        ("rainGauge", string_value("traditional-preferred")),
    ]);
    let fingerprint = source_fingerprint(
        &adapter_config,
        latitude,
        longitude,
        site_key,
        &timezone,
        provider_key,
        &source_key,
        SourceKind::PhysicalSensor,
        &key,
        version,
    )?;

    Some(EcowittStationConfiguration {
        active: boolean(field(station, "active")?)?,
        adapterConfig: adapter_config,
        cadenceSeconds: bounded_integer(field(station, "cadenceSeconds")?, 60, 3_600)?,
        displayName: nonempty_bounded_string(field(station, "displayName")?, 160)?,
        expectedMac: expected_mac,
        fingerprint,
        gatewayHost: gateway_host,
        key,
        latitude,
        longitude,
        model: nonempty_bounded_string(field(station, "model")?, 128)?,
        sourceKey: source_key,
        timezone,
    })
}

/// Parse and derive one Tempest station identity.
fn parse_tempest_station(
    value: &JsonValue,
    provider_key: &str,
    site_key: &str,
    version: u64,
) -> Option<TempestStationConfiguration> {
    let station = object(value)?;
    let key = stable_key(field(station, "key")?)?;
    let location_id = bounded_integer(field(station, "locationId")?, 1, 9_007_199_254_740_991)?;
    let device_id = bounded_integer(field(station, "deviceId")?, 1, 9_007_199_254_740_991)?;
    let latitude = finite_number(field(station, "latitude")?)?;
    let longitude = finite_number(field(station, "longitude")?)?;
    validate_coordinates(latitude, longitude)?;
    let timezone = time_zone(field(station, "timezone")?)?;
    let supersedes_source_key = format!("tempest-{location_id}-observations-v1");
    let source_key = format!("tempest-{location_id}-observations-v2");
    let adapter_config = object_value([
        (
            "contractVersion",
            string_value(TEMPEST_SOURCE_CONTRACT_VERSION),
        ),
        ("deviceId", JsonValue::Number(device_id as f64)),
        ("locationId", JsonValue::Number(location_id as f64)),
        (
            "sample",
            string_value("every-distinct-provider-observation"),
        ),
        ("supersedesSourceKey", string_value(&supersedes_source_key)),
    ]);
    let fingerprint = source_fingerprint(
        &adapter_config,
        latitude,
        longitude,
        site_key,
        &timezone,
        provider_key,
        &source_key,
        SourceKind::PhysicalSensor,
        &key,
        version,
    )?;

    Some(TempestStationConfiguration {
        active: boolean(field(station, "active")?)?,
        adapterConfig: adapter_config,
        cadenceSeconds: bounded_integer(field(station, "cadenceSeconds")?, 3_600, 86_400)?,
        deviceId: device_id,
        displayName: nonempty_bounded_string(field(station, "displayName")?, 160)?,
        fingerprint,
        historyStartDate: calendar_date(field(station, "historyStartDate")?)?,
        key,
        latitude,
        locationId: location_id,
        longitude,
        serial: nonempty_bounded_string(field(station, "serial")?, 128)?,
        sourceKey: source_key,
        timezone,
    })
}

/// Hash the recursively sorted source material without archive framing.
#[allow(clippy::too_many_arguments)]
fn source_fingerprint(
    adapter_config: &JsonValue,
    latitude: f64,
    longitude: f64,
    site_key: &str,
    timezone: &str,
    provider_key: &str,
    source_key: &str,
    source_kind: SourceKind,
    station_key: &str,
    version: u64,
) -> Option<String> {
    let material = object_value([
        ("adapterConfig", adapter_config.clone()),
        (
            "location",
            object_value([
                ("latitude", JsonValue::Number(latitude)),
                ("longitude", JsonValue::Number(longitude)),
                ("siteKey", string_value(site_key)),
                ("timezone", string_value(timezone)),
            ]),
        ),
        ("providerKey", string_value(provider_key)),
        ("sourceKey", string_value(source_key)),
        ("sourceKind", string_value(source_kind_wire(source_kind))),
        ("stationKey", string_value(station_key)),
        ("version", JsonValue::Number(version as f64)),
    ]);
    let mut canonical =
        ArchiveWirePrimitives::encode_sorted_json(&NativeWirePrimitives, &material)?;

    // remove the archive primitive's framing byte because canonicalizeJson has none
    if canonical.pop() != Some(b'\n') {
        return None;
    }
    Some(lower_hex(RevisionArchiveCanonicalCodec::sha256(
        &NativeWirePrimitives,
        &canonical,
    )))
}

/// Decode one complete JSON object.
fn decode_object(bytes: &[u8]) -> Option<JsonValue> {
    let value = ArchiveWirePrimitives::decode_json(&NativeWirePrimitives, bytes)?;
    object(&value)?;
    Some(value)
}

/// Borrow one JSON object.
fn object(value: &JsonValue) -> Option<&JsonValue> {
    // reject arrays and primitives at object boundaries
    if !matches!(value, JsonValue::Object(_)) {
        return None;
    }
    Some(value)
}

/// Borrow one JSON array.
fn array(value: &JsonValue) -> Option<&[JsonValue]> {
    // preserve only actual arrays without coercion
    match value {
        JsonValue::Array(values) => Some(values),
        _ => None,
    }
}

/// Admit the initial numeric catalog version.
fn exact_version(value: &JsonValue) -> Option<u64> {
    matches!(value, JsonValue::Number(number) if *number == 1.0).then_some(1)
}

/// Decode one exact JSON boolean.
fn boolean(value: &JsonValue) -> Option<bool> {
    // reject truthy coercion
    match value {
        JsonValue::Boolean(value) => Some(*value),
        _ => None,
    }
}

/// Decode one finite JSON number.
fn finite_number(value: &JsonValue) -> Option<f64> {
    // retain only the finite numeric alternative
    match value {
        JsonValue::Number(value) if value.is_finite() => Some(*value),
        _ => None,
    }
}

/// Decode one safe unsigned integer within the requested inclusive bounds.
fn bounded_integer(value: &JsonValue, minimum: u64, maximum: u64) -> Option<u64> {
    let number = finite_number(value)?;

    // mirror Number.isSafeInteger before converting to the generated unsigned type
    if number.fract() != 0.0
        || number < minimum as f64
        || number > maximum as f64
        || number > MAXIMUM_SAFE_INTEGER
    {
        return None;
    }
    Some(number as u64)
}

/// Decode one UTF-16 JSON string without replacement.
fn string(value: &JsonValue) -> Option<(&[u16], String)> {
    // generated Rust strings cannot retain lone UTF-16 surrogates
    match value {
        JsonValue::String(units) => Some((units, String::from_utf16(units).ok()?)),
        _ => None,
    }
}

/// Decode one non-empty string bounded by JavaScript UTF-16 length.
fn nonempty_bounded_string(value: &JsonValue, maximum: usize) -> Option<String> {
    let (units, value) = string(value)?;

    // match JavaScript trim and length checks without altering the returned value
    if units.len() > maximum || units.iter().copied().all(js_trim_code_unit) {
        return None;
    }
    Some(value)
}

/// Decode an optional bounded string, treating missing and null as null.
fn optional_bounded_string(value: Option<&JsonValue>, maximum: usize) -> Option<Nullable<String>> {
    // preserve both undefined and null as the generated null alternative
    match value {
        None | Some(JsonValue::Null) => Some(Nullable::Null),
        Some(value) => Some(Nullable::Value(nonempty_bounded_string(value, maximum)?)),
    }
}

/// Validate one lowercase kebab-case durable key.
fn stable_key(value: &JsonValue) -> Option<String> {
    let (units, value) = string(value)?;

    // enforce the original 80-code-unit portable key bound
    if units.is_empty() || units.len() > 80 {
        return None;
    }
    let bytes = value.as_bytes();

    // require alphanumeric segment edges and lowercase ASCII content
    if !bytes.first().is_some_and(u8::is_ascii_lowercase_or_digit)
        || !bytes.last().is_some_and(u8::is_ascii_lowercase_or_digit)
    {
        return None;
    }
    let mut previous_hyphen = false;
    // reject characters and repeated separators outside the TypeScript regular expression
    for byte in bytes {
        if byte.is_ascii_lowercase() || byte.is_ascii_digit() {
            previous_hyphen = false;
        } else if *byte == b'-' && !previous_hyphen {
            previous_hyphen = true;
        } else {
            return None;
        }
    }
    Some(value)
}

/// Validate one supported IANA timezone identity against the system catalog.
fn time_zone(value: &JsonValue) -> Option<String> {
    let (units, value) = string(value)?;

    // preserve the original 64-code-unit bound before consulting zoneinfo
    if units.is_empty() || units.len() > 64 || !system_time_zone_exists(&value) {
        return None;
    }
    Some(value)
}

/// Resolve one IANA name case-insensitively as Intl does.
fn system_time_zone_exists(value: &str) -> bool {
    let mut current = PathBuf::from("/usr/share/zoneinfo");

    // resolve each safe component without allowing the name to leave zoneinfo
    for component in value.split('/') {
        // reject empty and traversal components before directory access
        if component.is_empty() || matches!(component, "." | "..") {
            return false;
        }
        let Ok(entries) = std::fs::read_dir(&current) else {
            return false;
        };
        let mut matched = None;
        // match the case-insensitive names accepted by Intl
        for entry in entries.flatten() {
            let name = entry.file_name();
            // retain the on-disk spelling for the next component
            if name
                .to_str()
                .is_some_and(|name| name.eq_ignore_ascii_case(component))
            {
                matched = Some(name);
                break;
            }
        }
        let Some(matched) = matched else {
            return false;
        };
        current.push(matched);
    }
    current.metadata().is_ok_and(|metadata| metadata.is_file())
}

/// Validate one latitude and longitude pair.
fn validate_coordinates(latitude: f64, longitude: f64) -> Option<()> {
    // enforce the domain's closed coordinate ranges
    if !(-90.0..=90.0).contains(&latitude) || !(-180.0..=180.0).contains(&longitude) {
        return None;
    }
    Some(())
}

/// Normalize one HTTP attribution URL with the approved URL primitive.
fn http_url(value: &JsonValue, maximum: Option<usize>, allow_credentials: bool) -> Option<String> {
    let (units, text) = string(value)?;

    // apply the Ecowitt and Tempest bounded-string rule when requested
    if maximum.is_some_and(|maximum| units.len() > maximum)
        || maximum.is_some() && units.iter().copied().all(js_trim_code_unit)
    {
        return None;
    }
    let parsed = Url::parse(&text).ok()?;

    // admit only HTTP schemes and preserve the site parser's credential behavior
    if !matches!(parsed.scheme(), "http" | "https")
        || !allow_credentials
            && (!parsed.username().is_empty()
                || parsed
                    .password()
                    .is_some_and(|password| !password.is_empty()))
    {
        return None;
    }
    Some(parsed.to_string())
}

/// Normalize one site adapter object through JSON.stringify and JSON.parse.
fn bounded_adapter_object(value: &JsonValue) -> Option<JsonValue> {
    object(value)?;
    let encoded = ArchiveWirePrimitives::encode_ordered_json(&NativeWirePrimitives, value)?;
    let serialized = std::str::from_utf8(encoded.strip_suffix(b"\n")?).ok()?;

    // enforce the original JavaScript string-length bound
    if serialized.encode_utf16().count() > 16_384 {
        return None;
    }
    ArchiveWirePrimitives::decode_json(&NativeWirePrimitives, &encoded)
}

/// Decode one closed source-kind value.
fn source_kind(value: &JsonValue) -> Option<SourceKind> {
    let (_, value) = string(value)?;

    // map only generated wire values
    match value.as_str() {
        "physical_sensor" => Some(SourceKind::PhysicalSensor),
        "model_current" => Some(SourceKind::ModelCurrent),
        "reanalysis" => Some(SourceKind::Reanalysis),
        "forecast" => Some(SourceKind::Forecast),
        "tide_observation" => Some(SourceKind::TideObservation),
        "tide_prediction" => Some(SourceKind::TidePrediction),
        _ => None,
    }
}

/// Render one generated source kind as its wire value.
const fn source_kind_wire(value: SourceKind) -> &'static str {
    // retain exact generated wire spelling
    match value {
        SourceKind::PhysicalSensor => "physical_sensor",
        SourceKind::ModelCurrent => "model_current",
        SourceKind::Reanalysis => "reanalysis",
        SourceKind::Forecast => "forecast",
        SourceKind::TideObservation => "tide_observation",
        SourceKind::TidePrediction => "tide_prediction",
    }
}

/// Decode one closed station-kind value.
fn station_kind(value: &JsonValue) -> Option<StationKind> {
    let (_, value) = string(value)?;

    // admit only the physical and virtual station kinds
    match value.as_str() {
        "physical" => Some(StationKind::Physical),
        "virtual" => Some(StationKind::Virtual),
        _ => None,
    }
}

/// Decode one closed source capability.
fn source_capability(value: &JsonValue) -> Option<SourceCapability> {
    let (_, value) = string(value)?;

    // admit only generated capability wire values
    match value.as_str() {
        "current" => Some(SourceCapability::Current),
        "historical" => Some(SourceCapability::Historical),
        "forecast" => Some(SourceCapability::Forecast),
        "stream" => Some(SourceCapability::Stream),
        _ => None,
    }
}

/// Validate one private dotted-quad gateway address.
fn private_ipv4(value: &JsonValue) -> Option<String> {
    let host = nonempty_bounded_string(value, 64)?;
    let parts = host.split('.').collect::<Vec<_>>();

    // require exactly four decimal octets
    if parts.len() != 4 {
        return None;
    }
    let mut octets = [0_u16; 4];
    // reject non-decimal, oversized, and overlong octets
    for (index, part) in parts.iter().enumerate() {
        if part.is_empty() || part.len() > 3 || !part.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        octets[index] = part.parse().ok()?;
        // reject values outside dotted-quad range
        if octets[index] > 255 {
            return None;
        }
    }
    // admit only the three original RFC1918 ranges
    if octets[0] != 10
        && !(octets[0] == 172 && (16..=31).contains(&octets[1]))
        && !(octets[0] == 192 && octets[1] == 168)
    {
        return None;
    }
    Some(host)
}

/// Validate and uppercase one canonical colon-delimited MAC address.
fn canonical_mac(value: &JsonValue) -> Option<String> {
    let mac = nonempty_bounded_string(value, 17)?.to_ascii_uppercase();
    let bytes = mac.as_bytes();

    // require exactly six two-digit hexadecimal groups
    if bytes.len() != 17 {
        return None;
    }
    // validate every byte and separator position
    for (index, byte) in bytes.iter().copied().enumerate() {
        let separator = index % 3 == 2;
        // preserve canonical colon notation without alternative separators
        if separator && byte != b':' || !separator && !byte.is_ascii_hexdigit() {
            return None;
        }
    }
    Some(mac)
}

/// Validate one exact Gregorian YYYY-MM-DD date.
fn calendar_date(value: &JsonValue) -> Option<String> {
    let (units, text) = string(value)?;
    let bytes = text.as_bytes();

    // require the exact ASCII date spelling
    if units.len() != 10
        || bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || !bytes[..4].iter().all(u8::is_ascii_digit)
        || !bytes[5..7].iter().all(u8::is_ascii_digit)
        || !bytes[8..].iter().all(u8::is_ascii_digit)
    {
        return None;
    }
    let date = text.parse::<Date>().ok()?;

    // reject calendar rollover and non-canonical spellings
    if date.to_string() != text {
        return None;
    }
    Some(text)
}

/// Test uniqueness for one stream of borrowed strings.
fn unique_strings<'a>(values: impl Iterator<Item = &'a str>) -> bool {
    let mut seen = HashSet::new();

    // stop at the first repeated durable identity
    for value in values {
        if !seen.insert(value) {
            return false;
        }
    }
    true
}

/// Test uniqueness for one stream of safe unsigned integers.
fn unique_u64(values: impl Iterator<Item = u64>) -> bool {
    let mut seen = HashSet::new();

    // stop at the first repeated provider identity
    for value in values {
        if !seen.insert(value) {
            return false;
        }
    }
    true
}

/// Construct one generated JSON string.
fn string_value(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Construct one generated JSON object in producer order.
fn object_value<const N: usize>(fields: [(&str, JsonValue); N]) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(key, value)| (key.encode_utf16().collect(), value))
            .collect(),
    )
}

/// Render one digest in lowercase hexadecimal.
fn lower_hex(digest: [u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(64);

    // emit exactly two hexadecimal digits for every digest byte
    for byte in digest {
        text.push(char::from(HEX[usize::from(byte >> 4)]));
        text.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    text
}

/// Recognize one ECMAScript trim code unit.
const fn js_trim_code_unit(unit: u16) -> bool {
    matches!(
        unit,
        0x0009..=0x000d
            | 0x0020
            | 0x00a0
            | 0x1680
            | 0x2000..=0x200a
            | 0x2028
            | 0x2029
            | 0x202f
            | 0x205f
            | 0x3000
            | 0xfeff
    )
}

/// Extend ASCII byte predicates with the stable-key segment rule.
trait StableKeyByte {
    /// Recognize one lowercase ASCII letter or digit.
    fn is_ascii_lowercase_or_digit(&self) -> bool;
}

impl StableKeyByte for u8 {
    /// Recognize one lowercase ASCII letter or digit.
    fn is_ascii_lowercase_or_digit(&self) -> bool {
        self.is_ascii_lowercase() || self.is_ascii_digit()
    }
}
