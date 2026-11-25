//! Complete producer admission for the fixed twelve-gauge rain target projection.

use std::collections::{HashMap, HashSet};

use weather_contracts::{AdjustmentRevisionProjectionKind, JsonValue};
use weather_domain::{
    MetricName, RAIN_COLLECTION_STATIONS, RainCollectionStation, validate_metric_value,
};

use crate::archive_wire::{
    ArchiveWireCodec, ArchiveWirePrimitives, array, exact_keys, field, hash, member, string, text,
};
use crate::private_revision_archive::{
    AdmittedPrivateRevisionProjection, PrivateRevisionProjectionDependency,
    PrivateRevisionProjectionShape,
};
use crate::producer_projection::{binary64, instant_field, integer, nullable_binary64};
use crate::revision_archive_canonical::{RevisionArchiveCanonicalCodec, lower_hex};

const FIXED_GAUGE_MAXIMUM_BYTES: usize = 3_500 * 1_024;
const MAXIMUM_SAFE_INTEGER: i64 = 9_007_199_254_740_991;
const MINUTE_MILLISECONDS: i64 = 60_000;
const CAPTURE_WINDOW_MILLISECONDS: i64 = 65 * MINUTE_MILLISECONDS;

const TOP_KEYS: &[&str] = &[
    "captureBodies",
    "contractVersion",
    "family",
    "logicalReceivedAt",
    "projectionKind",
    "rows",
    "validAt",
];
const BODY_KEYS: &[&str] = &["bodyBase64", "bodySha256", "claims"];
const CLAIM_KEYS: &[&str] = &[
    "claimId",
    "completedAt",
    "stationId",
    "windowEndExclusive",
    "windowStart",
];
const ROW_KEYS: &[&str] = &[
    "captureMembers",
    "intervals",
    "normalizedRecord",
    "stationId",
    "storedContentSha256",
];
const MEMBER_KEYS: &[&str] = &["bodySha256", "claimId", "completedAt"];
const INTERVAL_KEYS: &[&str] = &[
    "bodySha256",
    "claimId",
    "completedAt",
    "durationMinutes",
    "precipitationMm64",
    "temperatureC64",
    "validAt",
];
const RECORD_KEYS: &[&str] = &[
    "metadata",
    "metrics",
    "productRunAt",
    "receivedAt",
    "sourceId",
    "sourceKind",
    "validAt",
];
const METRIC_KEYS: &[&str] = &[
    "apparentTemperatureC",
    "blackGlobeTemperatureC",
    "cloudCoverPercent",
    "pm25MicrogramsPerCubicMeter",
    "precipitationMm",
    "precipitationRateMmPerHour",
    "pressureHpa",
    "relativeHumidityPercent",
    "soilElectricalConductivityMicrosiemensPerCm",
    "soilMoisturePercent",
    "solarRadiationWm2",
    "temperatureC",
    "uvIndex",
    "waterLevelM",
    "wetBulbGlobeTemperatureC",
    "windDirectionDegrees",
    "windGustMps",
    "windSpeedMps",
];
const UNRELATED_METRIC_KEYS: &[&str] = &[
    "apparentTemperatureC",
    "blackGlobeTemperatureC",
    "cloudCoverPercent",
    "pm25MicrogramsPerCubicMeter",
    "pressureHpa",
    "relativeHumidityPercent",
    "soilElectricalConductivityMicrosiemensPerCm",
    "soilMoisturePercent",
    "solarRadiationWm2",
    "uvIndex",
    "waterLevelM",
    "wetBulbGlobeTemperatureC",
    "windDirectionDegrees",
    "windGustMps",
    "windSpeedMps",
];

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct ClaimKey {
    body_sha256: String,
    claim_id: Vec<u16>,
}

#[derive(Clone, Debug)]
struct CaptureClaim {
    completed_at: String,
    station_id: u64,
}

#[derive(Debug)]
struct SelectedClaims {
    claims: HashMap<ClaimKey, CaptureClaim>,
    members: Vec<(ClaimKey, String)>,
    last_completed_at: String,
}

#[derive(Clone, Copy, Debug)]
struct AdmittedInterval {
    duration_minutes: i64,
    precipitation_mm: f64,
    valid_at_milliseconds: i64,
}

/// Admit one complete canonical fixed-gauge target before archive publication.
pub(crate) fn admit_fixed_gauge_projection<P: ArchiveWirePrimitives>(
    codec: &ArchiveWireCodec<P>,
    value: &JsonValue,
    bytes: &[u8],
) -> Option<AdmittedPrivateRevisionProjection> {
    // require the source's independent cap and recursively sorted LF framing
    if !(2..=FIXED_GAUGE_MAXIMUM_BYTES).contains(&bytes.len())
        || codec.encode(value)?.as_slice() != bytes
        || !exact_keys(value, TOP_KEYS)
        || text(value, "contractVersion")? != "adjustment-rain-fixed-gauge-target-projection/v1"
        || text(value, "family")? != "rain"
        || text(value, "projectionKind")? != "target_revision"
    {
        return None;
    }

    let (logical_received_at, _) = strict_instant(codec, value, "logicalReceivedAt")?;
    let (valid_at, valid_at_milliseconds) = strict_instant(codec, value, "validAt")?;
    let claims = validate_capture_bodies(codec, value, valid_at_milliseconds)?;
    let rows = array(value, "rows")?;
    // retain the fixed catalog's exact twelve-row order
    if rows.len() != RAIN_COLLECTION_STATIONS.len() {
        return None;
    }

    let mut referenced_claims = HashSet::new();
    let mut latest_completed_at: Option<String> = None;
    // bind every derived row to its corresponding physical gauge and capture graph
    for (row, station) in rows.iter().zip(RAIN_COLLECTION_STATIONS) {
        if !exact_keys(row, ROW_KEYS)
            || integer(field(row, "stationId")?, 0, MAXIMUM_SAFE_INTEGER)? as u64
                != station.location_id
        {
            return None;
        }
        let selected = validate_capture_members(codec, row, station.location_id, &claims)?;
        // account for every selected claim in the document-wide capture graph
        for (key, completed_at) in &selected.members {
            referenced_claims.insert(key.clone());
            // retain the latest canonical completion clock
            if latest_completed_at
                .as_ref()
                .is_none_or(|latest| completed_at > latest)
            {
                latest_completed_at = Some(completed_at.clone());
            }
        }
        let intervals = validate_intervals(codec, row, &selected.claims, valid_at_milliseconds)?;
        validate_normalized_record(
            codec,
            row,
            station,
            &valid_at,
            valid_at_milliseconds,
            &selected.last_completed_at,
            &intervals,
        )?;
    }

    // prohibit unused captures and bind availability to the final member completion
    if referenced_claims.len() != claims.len()
        || claims.keys().any(|key| !referenced_claims.contains(key))
        || latest_completed_at.as_deref() != Some(logical_received_at.as_str())
    {
        return None;
    }

    Some(AdmittedPrivateRevisionProjection {
        projection_kind: AdjustmentRevisionProjectionKind::TargetRevision,
        shape: PrivateRevisionProjectionShape::Grouped {
            row_count: rows.len(),
        },
        dependency: PrivateRevisionProjectionDependency::None,
    })
}

/// Validate and index every exact raw provider body and capture claim.
fn validate_capture_bodies<P: ArchiveWirePrimitives>(
    codec: &ArchiveWireCodec<P>,
    value: &JsonValue,
    valid_at_milliseconds: i64,
) -> Option<HashMap<ClaimKey, CaptureClaim>> {
    let bodies = array(value, "captureBodies")?;
    // require at least one independently replayable raw capture body
    if bodies.is_empty() {
        return None;
    }
    let mut admitted = HashMap::new();
    let mut previous_body_sha256: Option<String> = None;
    // preserve strict body-address order independently of capture completion order
    for body in bodies {
        if !exact_keys(body, BODY_KEYS) {
            return None;
        }
        let body_sha256 = hash(body, "bodySha256")?;
        // exclude duplicate and descending body identities
        if previous_body_sha256
            .as_ref()
            .is_some_and(|previous| body_sha256 <= *previous)
        {
            return None;
        }
        previous_body_sha256 = Some(body_sha256.clone());
        let body_base64 = text(body, "bodyBase64")?;
        // preserve the original minimum spelling before exact padded base64 admission
        if body_base64.len() < 4 {
            return None;
        }
        let raw = codec.base64_decode(&body_base64)?;
        // authenticate the complete decoded provider response
        if lower_hex(codec.sha256(&raw)) != body_sha256 {
            return None;
        }
        let claims = array(body, "claims")?;
        // prohibit inert raw bodies without an authenticated claim
        if claims.is_empty() {
            return None;
        }
        let mut previous_claim_order: Option<Vec<u16>> = None;
        // validate every claim carried by this deduplicated body
        for claim in claims {
            if !exact_keys(claim, CLAIM_KEYS) {
                return None;
            }
            let claim_id = claim_id(field(claim, "claimId")?)?.to_vec();
            let (completed_at, _) = strict_instant(codec, claim, "completedAt")?;
            let (_, window_start) = strict_instant(codec, claim, "windowStart")?;
            let (_, window_end) = strict_instant(codec, claim, "windowEndExclusive")?;
            let station_id = integer(field(claim, "stationId")?, 0, MAXIMUM_SAFE_INTEGER)? as u64;
            let completed_at_units = completed_at.encode_utf16().collect::<Vec<_>>();
            let order = joined_order(&[&completed_at_units, &claim_id]);
            // retain only real catalog windows overlapping the target-hour tolerance
            if !RAIN_COLLECTION_STATIONS
                .iter()
                .any(|station| station.location_id == station_id)
                || window_start >= window_end
                || window_start > valid_at_milliseconds
                || window_end <= valid_at_milliseconds - CAPTURE_WINDOW_MILLISECONDS
                || previous_claim_order
                    .as_ref()
                    .is_some_and(|previous| order <= *previous)
            {
                return None;
            }
            previous_claim_order = Some(order);
            let key = ClaimKey {
                body_sha256: body_sha256.clone(),
                claim_id,
            };
            // reject repeated body-claim addresses globally
            if admitted
                .insert(
                    key,
                    CaptureClaim {
                        completed_at,
                        station_id,
                    },
                )
                .is_some()
            {
                return None;
            }
        }
    }
    Some(admitted)
}

/// Validate one station row's complete sorted raw-capture membership.
fn validate_capture_members<P: ArchiveWirePrimitives>(
    codec: &ArchiveWireCodec<P>,
    row: &JsonValue,
    station_id: u64,
    claims: &HashMap<ClaimKey, CaptureClaim>,
) -> Option<SelectedClaims> {
    let members = array(row, "captureMembers")?;
    // require raw capture evidence for every fixed gauge
    if members.is_empty() {
        return None;
    }
    let mut selected = HashMap::new();
    let mut retained_members = Vec::with_capacity(members.len());
    let mut previous_order: Option<Vec<u16>> = None;
    // bind strict member order to the exact deduplicated body claims
    for member_value in members {
        if !exact_keys(member_value, MEMBER_KEYS) {
            return None;
        }
        let body_sha256 = hash(member_value, "bodySha256")?;
        let claim_id = claim_id(field(member_value, "claimId")?)?.to_vec();
        let (completed_at, _) = strict_instant(codec, member_value, "completedAt")?;
        let completed_at_units = completed_at.encode_utf16().collect::<Vec<_>>();
        let body_sha256_units = body_sha256.encode_utf16().collect::<Vec<_>>();
        let order = joined_order(&[&completed_at_units, &claim_id, &body_sha256_units]);
        let key = ClaimKey {
            body_sha256,
            claim_id,
        };
        let claim = claims.get(&key)?;
        // prohibit foreign, rewritten, repeated, and out-of-order membership
        if claim.station_id != station_id
            || claim.completed_at != completed_at
            || selected.contains_key(&key)
            || previous_order
                .as_ref()
                .is_some_and(|previous| order <= *previous)
        {
            return None;
        }
        previous_order = Some(order);
        selected.insert(key.clone(), claim.clone());
        retained_members.push((key, completed_at));
    }
    let last_completed_at = retained_members.last()?.1.clone();
    Some(SelectedClaims {
        claims: selected,
        members: retained_members,
        last_completed_at,
    })
}

/// Validate every successful minute interval selected by the frozen recipe.
fn validate_intervals<P: ArchiveWirePrimitives>(
    codec: &ArchiveWireCodec<P>,
    row: &JsonValue,
    claims: &HashMap<ClaimKey, CaptureClaim>,
    valid_at_milliseconds: i64,
) -> Option<Vec<AdmittedInterval>> {
    let values = array(row, "intervals")?;
    let mut admitted = Vec::with_capacity(values.len());
    let mut previous_valid_at: Option<String> = None;
    // preserve chronological interval order without duplicate endpoints
    for interval in values {
        if !exact_keys(interval, INTERVAL_KEYS) {
            return None;
        }
        let body_sha256 = hash(interval, "bodySha256")?;
        let claim_id = claim_id(field(interval, "claimId")?)?.to_vec();
        let (completed_at, _) = strict_instant(codec, interval, "completedAt")?;
        let (interval_valid_at, interval_milliseconds) =
            strict_instant(codec, interval, "validAt")?;
        let duration_minutes = integer(field(interval, "durationMinutes")?, 1, 5)?;
        let key = ClaimKey {
            body_sha256,
            claim_id,
        };
        let claim = claims.get(&key)?;
        // bind each interval to one selected claim and target-window endpoint
        if claim.completed_at != completed_at
            || previous_valid_at
                .as_ref()
                .is_some_and(|previous| interval_valid_at <= *previous)
            || interval_milliseconds > valid_at_milliseconds
            || interval_milliseconds < valid_at_milliseconds - CAPTURE_WINDOW_MILLISECONDS
        {
            return None;
        }
        previous_valid_at = Some(interval_valid_at);
        let precipitation_mm = binary64(field(interval, "precipitationMm64")?)?;
        // exclude negative interval amounts while preserving positive and negative zero parity
        if precipitation_mm < 0.0 {
            return None;
        }
        nullable_binary64(field(interval, "temperatureC64")?)?;
        admitted.push(AdmittedInterval {
            duration_minutes,
            precipitation_mm,
            valid_at_milliseconds: interval_milliseconds,
        });
    }
    Some(admitted)
}

/// Bind one derived hourly record to its gauge, target clock, and selected intervals.
fn validate_normalized_record<P: ArchiveWirePrimitives>(
    codec: &ArchiveWireCodec<P>,
    row: &JsonValue,
    station: &RainCollectionStation,
    valid_at: &str,
    valid_at_milliseconds: i64,
    last_completed_at: &str,
    intervals: &[AdmittedInterval],
) -> Option<()> {
    let stored_content_sha256 = hash(row, "storedContentSha256")?;
    let record = field(row, "normalizedRecord")?;
    if !exact_keys(record, RECORD_KEYS)
        || text(record, "sourceKind")? != "physical_sensor"
        || !matches!(field(record, "productRunAt")?, JsonValue::Null)
    {
        return None;
    }
    let record_valid_at = codec.normalize_explicit_utc_instant(&text(record, "validAt")?)?;
    let received_at = codec.normalize_explicit_utc_instant(&text(record, "receivedAt")?)?;
    // bind the normalized clock fields to the target and final capture member
    if record_valid_at != valid_at || received_at != last_completed_at {
        return None;
    }
    source_id(field(record, "sourceId")?)?;
    let complete = !intervals.is_empty();
    // require the exact derived physical-gauge metadata rather than an allowlisted superset
    if field(record, "metadata")? != &target_metadata(station, complete) {
        return None;
    }
    let metrics = field(record, "metrics")?;
    // retain the complete canonical metric shape and explicit unrelated nulls
    if !exact_keys(metrics, METRIC_KEYS)
        || UNRELATED_METRIC_KEYS
            .iter()
            .any(|key| !matches!(field(metrics, key), Some(JsonValue::Null)))
    {
        return None;
    }
    let precipitation = nullable_metric(metrics, MetricName::PrecipitationMm)?;
    let precipitation_rate = nullable_metric(metrics, MetricName::PrecipitationRateMmPerHour)?;
    nullable_metric(metrics, MetricName::TemperatureC)?;
    let amount = complete_tiling_amount(intervals, valid_at_milliseconds)?;
    // complete rows equal the exact sequential sixty-minute interval sum
    if let Some(amount) = amount {
        if precipitation != Some(amount) || precipitation_rate != Some(amount) {
            return None;
        }
    // incomplete coverage stays explicit and never becomes a synthetic zero
    } else if precipitation.is_some() || precipitation_rate.is_some() || !intervals.is_empty() {
        return None;
    }

    let content = JsonValue::Object(vec![
        member("metadata", field(record, "metadata")?.clone()),
        member("metrics", metrics.clone()),
        member("productRunAt", JsonValue::Null),
        member("sourceId", field(record, "sourceId")?.clone()),
        member("sourceKind", string("physical_sensor")),
        member("validAt", string(valid_at)),
    ]);
    let encoded = codec.encode(&content)?;
    let content_bytes = encoded.strip_suffix(b"\n")?;
    // hash the original canonical content without receivedAt or LF framing
    if lower_hex(codec.sha256(content_bytes)) != stored_content_sha256 {
        return None;
    }
    Some(())
}

/// Require one exact backward sixty-minute tiling and endpoint tolerance.
fn complete_tiling_amount(
    intervals: &[AdmittedInterval],
    valid_at_milliseconds: i64,
) -> Option<Option<f64>> {
    // preserve the source's explicit incomplete sentinel
    if intervals.is_empty() {
        return Some(None);
    }
    let mut elapsed_minutes = 0i64;
    let mut previous_end: Option<i64> = None;
    let mut amount = 0.0;
    // walk exact chronological tiles without interpolation or proration
    for interval in intervals {
        let start = interval
            .valid_at_milliseconds
            .checked_sub(interval.duration_minutes.checked_mul(MINUTE_MILLISECONDS)?)?;
        // require every tile to begin at the preceding endpoint
        if previous_end.is_some_and(|end| start != end) {
            return None;
        }
        previous_end = Some(interval.valid_at_milliseconds);
        elapsed_minutes = elapsed_minutes.checked_add(interval.duration_minutes)?;
        amount += interval.precipitation_mm;
    }
    let final_end = previous_end?;
    let offset = valid_at_milliseconds.checked_sub(final_end)?;
    // preserve the exact hour width and integral zero-to-five-minute endpoint lag
    if elapsed_minutes != 60
        || offset % MINUTE_MILLISECONDS != 0
        || !(0..=5).contains(&(offset / MINUTE_MILLISECONDS))
    {
        return None;
    }
    Some(Some(amount))
}

/// Build the exact closed metadata produced for one physical Tempest gauge.
fn target_metadata(station: &RainCollectionStation, complete: bool) -> JsonValue {
    // select the only quality status admitted for the retained coverage shape
    let status = if complete {
        "complete_interval_coverage"
    } else {
        "incomplete_interval_coverage"
    };
    JsonValue::Object(vec![
        member(
            "device",
            JsonValue::Object(vec![
                member("model", string("Tempest")),
                member("serial", string(station.serial)),
                member("vendor", string("WeatherFlow")),
            ]),
        ),
        member("model", JsonValue::Null),
        member(
            "provider",
            JsonValue::Object(vec![
                member("dataset", string("rain_fixed_gauge_hourly")),
                member("device_id", JsonValue::Number(station.device_id as f64)),
                member("location_id", JsonValue::Number(station.location_id as f64)),
                member("report_interval_minutes", JsonValue::Number(60.0)),
            ]),
        ),
        member(
            "quality",
            JsonValue::Object(vec![
                member("sampling", string("backward_exact_interval_tiling")),
                member("status", string(status)),
            ]),
        ),
        member("upstreamTimezone", string("America/Los_Angeles")),
    ])
}

/// Read and validate one nullable canonical metric from the complete metric object.
fn nullable_metric(value: &JsonValue, metric: MetricName) -> Option<Option<f64>> {
    match field(value, metric.wire_value())? {
        JsonValue::Null => Some(None),
        JsonValue::Number(number) => {
            validate_metric_value(metric, *number).ok()?;
            Some(Some(*number))
        }
        _ => None,
    }
}

/// Require the producer's canonical 20xx millisecond UTC clock.
fn strict_instant(
    codec: &impl RevisionArchiveCanonicalCodec,
    value: &JsonValue,
    name: &str,
) -> Option<(String, i64)> {
    let clock = text(value, name)?;
    // narrow the shared four-digit grammar to the fixed-gauge source's 20xx regex
    if !clock.starts_with("20") {
        return None;
    }
    Some((clock, instant_field(codec, value, name)?))
}

/// Borrow one bounded immutable claim identity with JavaScript length semantics.
fn claim_id(value: &JsonValue) -> Option<&[u16]> {
    let JsonValue::String(units) = value else {
        return None;
    };
    (1..=128).contains(&units.len()).then_some(units)
}

/// Admit one bounded nonblank source identity with JavaScript trim semantics.
fn source_id(value: &JsonValue) -> Option<&[u16]> {
    let JsonValue::String(units) = value else {
        return None;
    };
    // reject empty, oversized, and all-ECMAScript-whitespace identities
    if units.is_empty() || units.len() > 128 || units.iter().all(|unit| js_trim_unit(*unit)) {
        return None;
    }
    Some(units)
}

/// Match every BMP code unit removed by ECMAScript String.prototype.trim.
const fn js_trim_unit(unit: u16) -> bool {
    matches!(
        unit,
        0x0009 | 0x000a | 0x000b | 0x000c | 0x000d | 0x0020 | 0x00a0 | 0x1680 | 0x2000
            ..=0x200a | 0x2028 | 0x2029 | 0x202f | 0x205f | 0x3000 | 0xfeff
    )
}

/// Join UTF-16 order components with the producer's unambiguous LF separator.
fn joined_order(parts: &[&[u16]]) -> Vec<u16> {
    let capacity = parts.iter().map(|part| part.len()).sum::<usize>() + parts.len() - 1;
    let mut joined = Vec::with_capacity(capacity);
    // preserve JavaScript template-string code-unit comparison exactly
    for (index, part) in parts.iter().enumerate() {
        // insert one separator between adjacent order components
        if index > 0 {
            joined.push(u16::from(b'\n'));
        }
        joined.extend_from_slice(part);
    }
    joined
}
