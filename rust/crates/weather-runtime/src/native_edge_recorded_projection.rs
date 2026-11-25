//! Native recorded-observation aggregation and solar-cloud evidence projection.

use std::error::Error;
use std::f64::consts::PI;
use std::fmt::{Display, Formatter};

use weather_contracts::JsonValue;
use weather_edge::{
    AdmittedForecastRecordedInstant, ArchiveWirePrimitives, ForecastRecordedInputMetrics,
    ForecastRecordedInputRecord, ForecastRecordedInputSourceKind, ForecastRecordedSolarSite,
};

use crate::NativeWirePrimitives;
use crate::native_edge_recorded_owner::NativeForecastRecordedProjection;

const HOUR_MS: i64 = 60 * 60 * 1_000;
const DAY_MS: i64 = 24 * HOUR_MS;
const PAIR_GAP_MS: i64 = 20 * 60 * 1_000;
const MAX_PAIR_COMPARISONS: usize = 64_000;
const MAX_WS90_AGE_MS: i64 = 5 * 60 * 1_000;
const MAX_REGIONAL_AGE_MS: i64 = 20 * 60 * 1_000;
const MIN_SOLAR_ELEVATION_DEGREES: f64 = 15.0;
const MIN_CLEAR_SKY_RADIATION_WM2: f64 = 200.0;
const CLEAR_SKY_TOLERANCE: f64 = 0.85;
const CLOUD_ATTENUATION_RANGE: f64 = 0.65;
const MAX_RADIATION_RATIO: f64 = 1.5;

/// Concrete source-compatible pure projection owner.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeForecastRecordedProjectionOwner;

impl NativeForecastRecordedProjection for NativeForecastRecordedProjectionOwner {
    type Error = NativeForecastRecordedProjectionError;

    /// Aggregate validated records into the exact hourly public point array.
    fn forecast_observation_points(
        &self,
        records: &[ForecastRecordedInputRecord],
        projection_from: &AdmittedForecastRecordedInstant,
        through: &AdmittedForecastRecordedInstant,
    ) -> Result<JsonValue, Self::Error> {
        forecast_observation_points(records, projection_from, through)
    }

    /// Select the newest viable bounded daylight evidence pair.
    fn solar_cloud_evidence(
        &self,
        records: &[ForecastRecordedInputRecord],
        site: ForecastRecordedSolarSite,
        through: &AdmittedForecastRecordedInstant,
    ) -> Result<Option<JsonValue>, Self::Error> {
        solar_cloud_evidence(records, site, through)
    }
}

/// Closed pure-projection refusal categories.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeForecastRecordedProjectionError {
    ForecastWindow,
    SolarHistoryLimit,
    SolarPairingLimit,
    Time,
}

impl Display for NativeForecastRecordedProjectionError {
    /// Render the original projection refusal category.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        // preserve the source algorithm boundaries
        match self {
            Self::ForecastWindow => formatter.write_str("Forecast observation window is invalid"),
            Self::SolarHistoryLimit => {
                formatter.write_str("Solar cloud evidence history limit is invalid")
            }
            Self::SolarPairingLimit => {
                formatter.write_str("Solar cloud evidence pairing limit exceeded")
            }
            Self::Time => formatter.write_str("Forecast recorded projection time is invalid"),
        }
    }
}

impl Error for NativeForecastRecordedProjectionError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Metric {
    Temperature,
    ApparentTemperature,
    WindSpeed,
    WindGust,
    WindDirection,
    PrecipitationRate,
    CloudCover,
    RelativeHumidity,
    Pm25,
    UvIndex,
    Pressure,
    PressureChange,
}

impl Metric {
    /// Return the exact public JSON key.
    const fn key(self) -> &'static str {
        // preserve the source metric spelling
        match self {
            Self::Temperature => "temperatureC",
            Self::ApparentTemperature => "apparentTemperatureC",
            Self::WindSpeed => "windSpeedMps",
            Self::WindGust => "windGustMps",
            Self::WindDirection => "windDirectionDegrees",
            Self::PrecipitationRate => "precipitationRateMmPerHour",
            Self::CloudCover => "cloudCoverPercent",
            Self::RelativeHumidity => "relativeHumidityPercent",
            Self::Pm25 => "pm25MicrogramsPerCubicMeter",
            Self::UvIndex => "uvIndex",
            Self::Pressure => "pressureHpa",
            Self::PressureChange => "pressureChange3hHpa",
        }
    }
}

const OBSERVATION_METRICS: [Metric; 11] = [
    Metric::Temperature,
    Metric::ApparentTemperature,
    Metric::WindSpeed,
    Metric::WindGust,
    Metric::WindDirection,
    Metric::PrecipitationRate,
    Metric::CloudCover,
    Metric::RelativeHumidity,
    Metric::Pm25,
    Metric::UvIndex,
    Metric::Pressure,
];

struct Revision<'a> {
    key: String,
    record: &'a ForecastRecordedInputRecord,
}

struct HourBin<'a> {
    hour: i64,
    rows: Vec<&'a ForecastRecordedInputRecord>,
    source_id: &'a str,
}

#[derive(Clone, Copy)]
struct Candidate<'a> {
    record: &'a ForecastRecordedInputRecord,
    samples: usize,
    value: f64,
}

struct MetricCandidates<'a> {
    metric: Metric,
    values: Vec<Candidate<'a>>,
}

/// Port the source Map-based hourly aggregation without introducing unordered maps.
fn forecast_observation_points(
    records: &[ForecastRecordedInputRecord],
    projection_from: &AdmittedForecastRecordedInstant,
    through: &AdmittedForecastRecordedInstant,
) -> Result<JsonValue, NativeForecastRecordedProjectionError> {
    let first = projection_from.epoch_ms;
    let cutoff = through.epoch_ms;
    // refuse unbounded or unaligned projection geometry
    if first.rem_euclid(HOUR_MS) != 0
        || cutoff < first
        || cutoff.saturating_sub(first) > 25 * HOUR_MS
    {
        return Err(NativeForecastRecordedProjectionError::ForecastWindow);
    }
    let mut revisions: Vec<Revision<'_>> = Vec::new();
    // retain each logical source-time key at its first insertion position
    for record in records {
        let valid = record.valid_at.epoch_ms;
        let received = record.received_at.epoch_ms;
        let physical = record.provenance.source_kind
            == ForecastRecordedInputSourceKind::PhysicalSensor
            && !record.provenance.source_key.starts_with("rain-target-")
            && if is_farm(record) {
                record.provenance.source_key == "ecowitt-88f15505d89f-local-live-v1"
            } else {
                record.provenance.station_slug != "ballydidean-ecowitt"
                    && matches!(
                        record.provenance.provider_key.as_str(),
                        "weatherflow-tempest"
                            | "ambient-weather-network"
                            | "weather-underground"
                            | "netatmo-weathermap"
                            | "purpleair"
                    )
            };
        let regional = record.provenance.source_kind
            == ForecastRecordedInputSourceKind::ModelCurrent
            && record.provenance.provider_key == "open-meteo"
            && record.provenance.station_slug == "open-meteo-virtual";
        // exclude unsupported sources and out-of-window context
        if (!physical && !regional)
            || valid < first.saturating_sub(3 * HOUR_MS)
            || valid >= cutoff
            || received > cutoff
        {
            continue;
        }
        let key = format!("{}/{valid}", record.provenance.source_id);
        // replace only with a genuinely newer revision
        if let Some(previous) = revisions.iter_mut().find(|entry| entry.key == key) {
            if previous.record.received_at.epoch_ms < received {
                previous.record = record;
            }
        } else {
            revisions.push(Revision { key, record });
        }
    }
    let mut bins: Vec<HourBin<'_>> = Vec::new();
    // retain source-local hourly bins in first insertion order
    for revision in &revisions {
        let record = revision.record;
        let hour = record.valid_at.epoch_ms.div_euclid(HOUR_MS) * HOUR_MS;
        // append to the already inserted source-hour bin
        if let Some(bin) = bins
            .iter_mut()
            .find(|bin| bin.source_id == record.provenance.source_id.as_str() && bin.hour == hour)
        {
            bin.rows.push(record);
        } else {
            bins.push(HourBin {
                hour,
                rows: vec![record],
                source_id: &record.provenance.source_id,
            });
        }
    }
    let mut points = Vec::new();
    let mut hour = first;
    // visit every elapsed hourly cell including a partial current hour
    while hour < cutoff {
        points.push(hourly_point(&bins, hour, cutoff, through)?);
        hour = hour.saturating_add(HOUR_MS);
    }
    Ok(JsonValue::Array(points))
}

/// Aggregate and select every metric independently for one labeled hour.
fn hourly_point(
    bins: &[HourBin<'_>],
    hour: i64,
    cutoff: i64,
    through: &AdmittedForecastRecordedInstant,
) -> Result<JsonValue, NativeForecastRecordedProjectionError> {
    let mut candidates: Vec<MetricCandidates<'_>> = Vec::new();
    // aggregate each exact source before station-tier selection
    for bin in bins {
        // retain scalar and preceding-hour gust bins only
        if bin.hour != hour && bin.hour != hour.saturating_sub(HOUR_MS) {
            continue;
        }
        let latest = latest_record(&bin.rows);
        // collect one aggregate for every usable metric
        for metric in OBSERVATION_METRICS {
            let metric_hour = if metric == Metric::WindGust {
                hour.saturating_sub(HOUR_MS)
            } else {
                hour
            };
            // keep gusts separate from labeled scalar bins
            if bin.hour != metric_hour {
                continue;
            }
            let values = bin
                .rows
                .iter()
                .filter_map(|record| usable_metric(&record.metrics, metric))
                .collect::<Vec<_>>();
            let value = aggregate_values(metric, &values);
            // missing metrics never disqualify other readings
            if let Some(value) = value {
                push_candidate(
                    &mut candidates,
                    metric,
                    Candidate {
                        record: latest,
                        samples: values.len(),
                        value,
                    },
                );
            }
        }
        // pressure changes stay within one source and four complete hourly bins
        if bin.hour == hour {
            let mut pressures = Vec::with_capacity(4);
            let mut sample_count = 0usize;
            // read the current and three preceding source-local bins
            for offset in 0..=3 {
                let target = hour.saturating_sub(i64::from(offset) * HOUR_MS);
                let rows = bins
                    .iter()
                    .find(|candidate| {
                        candidate.source_id == bin.source_id && candidate.hour == target
                    })
                    .map_or(&[][..], |candidate| candidate.rows.as_slice());
                let values = rows
                    .iter()
                    .filter_map(|record| usable_metric(&record.metrics, Metric::Pressure))
                    .collect::<Vec<_>>();
                sample_count = sample_count.saturating_add(values.len());
                pressures.push(mean(&values));
            }
            // require all four hourly pressure bins
            if pressures.iter().all(Option::is_some) {
                let value = pressures[0].expect("checked pressure")
                    - pressures[3].expect("checked pressure");
                // reject impossible tendencies independently of scalar pressure
                if value.abs() <= 20.0 {
                    push_candidate(
                        &mut candidates,
                        Metric::PressureChange,
                        Candidate {
                            record: latest,
                            samples: sample_count,
                            value,
                        },
                    );
                }
            }
        }
    }
    let mut metrics = Vec::new();
    let mut pressure_change = None;
    let mut sources = Vec::new();
    let mut counts = Vec::new();
    // apply source-tier precedence separately for each candidate metric
    for available in &mut candidates {
        available.values.sort_by(|left, right| {
            right
                .record
                .valid_at
                .epoch_ms
                .cmp(&left.record.valid_at.epoch_ms)
                .then_with(|| {
                    left.record
                        .provenance
                        .source_key
                        .cmp(&right.record.provenance.source_key)
                })
        });
        let farm = available
            .values
            .iter()
            .copied()
            .find(|sample| is_farm(sample.record));
        let mut stations: Vec<(&str, Candidate<'_>)> = Vec::new();
        // collapse duplicate feeds into one newest station aggregate
        for sample in &available.values {
            let station = sample.record.provenance.station_slug.as_str();
            // exclude the farm and regional rows from local station votes
            if sample.record.provenance.source_kind
                == ForecastRecordedInputSourceKind::PhysicalSensor
                && !is_farm(sample.record)
                && !stations.iter().any(|(candidate, _)| *candidate == station)
            {
                stations.push((station, *sample));
            }
        }
        let regional = available.values.iter().copied().find(|sample| {
            sample.record.provenance.source_kind == ForecastRecordedInputSourceKind::ModelCurrent
        });
        let selected = if let Some(farm) = farm {
            vec![farm]
        } else if !stations.is_empty() {
            stations.iter().map(|(_, sample)| *sample).collect()
        } else {
            regional.into_iter().collect()
        };
        let selected_values = selected
            .iter()
            .map(|sample| sample.value)
            .collect::<Vec<_>>();
        let value = aggregate_values(available.metric, &selected_values);
        let Some(value) = value else {
            continue;
        };
        // retain pressure tendency outside the scalar metrics object
        if available.metric == Metric::PressureChange {
            pressure_change = Some(value);
        } else {
            metrics.push((available.metric.key(), JsonValue::Number(value)));
        }
        let tier = if farm.is_some() {
            "ballydidean"
        } else if !stations.is_empty() {
            "local_mean"
        } else {
            "regional"
        };
        sources.push((available.metric.key(), text(tier)));
        counts.push((
            available.metric.key(),
            object(vec![
                (
                    "samples",
                    JsonValue::Number(
                        selected.iter().map(|sample| sample.samples).sum::<usize>() as f64
                    ),
                ),
                ("stations", JsonValue::Number(selected.len() as f64)),
            ]),
        ));
    }
    let valid_at = NativeWirePrimitives
        .format_utc_milliseconds(hour)
        .ok_or(NativeForecastRecordedProjectionError::Time)?;
    Ok(object(vec![
        ("validAt", text(&valid_at)),
        ("metrics", object(metrics)),
        (
            "pressureChange3hHpa",
            pressure_change.map_or(JsonValue::Null, JsonValue::Number),
        ),
        ("sources", object(sources)),
        ("counts", object(counts)),
        ("recordedThrough", text(&through.value)),
        (
            "partial",
            JsonValue::Boolean(hour.saturating_add(HOUR_MS) > cutoff),
        ),
    ]))
}

/// Preserve candidate insertion order while appending one metric value.
fn push_candidate<'a>(
    candidates: &mut Vec<MetricCandidates<'a>>,
    metric: Metric,
    candidate: Candidate<'a>,
) {
    // append to the first inserted metric bucket
    if let Some(available) = candidates
        .iter_mut()
        .find(|available| available.metric == metric)
    {
        available.values.push(candidate);
    } else {
        candidates.push(MetricCandidates {
            metric,
            values: vec![candidate],
        });
    }
}

/// Select the stable newest-valid row within one source bin.
fn latest_record<'a>(rows: &[&'a ForecastRecordedInputRecord]) -> &'a ForecastRecordedInputRecord {
    let mut latest = rows[0];
    // retain the first row on equal valid timestamps
    for row in &rows[1..] {
        if row.valid_at.epoch_ms > latest.valid_at.epoch_ms {
            latest = row;
        }
    }
    latest
}

/// Identify the single farm gateway source.
fn is_farm(record: &ForecastRecordedInputRecord) -> bool {
    record.provenance.provider_key == "ecowitt-local"
        && record.provenance.station_slug == "ballydidean-ecowitt"
}

/// Read and validate one metric against the source's normalized domains.
fn usable_metric(metrics: &ForecastRecordedInputMetrics, metric: Metric) -> Option<f64> {
    let (value, minimum, maximum, maximum_exclusive) = match metric {
        Metric::Temperature => (metrics.temperature_c?, -100.0, 70.0, false),
        Metric::ApparentTemperature => (metrics.apparent_temperature_c?, -100.0, 70.0, false),
        Metric::WindSpeed => (metrics.wind_speed_mps?, 0.0, 150.0, false),
        Metric::WindGust => (metrics.wind_gust_mps?, 0.0, 150.0, false),
        Metric::WindDirection => (metrics.wind_direction_degrees?, 0.0, 360.0, true),
        Metric::PrecipitationRate => (
            metrics.precipitation_rate_mm_per_hour?,
            0.0,
            10_000.0,
            false,
        ),
        Metric::CloudCover => (metrics.cloud_cover_percent?, 0.0, 100.0, false),
        Metric::RelativeHumidity => (metrics.relative_humidity_percent?, 0.0, 100.0, false),
        Metric::Pm25 => (metrics.pm25_micrograms_per_cubic_meter?, 0.0, 999.0, false),
        Metric::UvIndex => (metrics.uv_index?, 0.0, 20.0, false),
        Metric::Pressure => (metrics.pressure_hpa?, 100.0, 1_200.0, false),
        Metric::PressureChange => return None,
    };
    // retain inclusive maxima except wind direction's full-revolution endpoint
    (value.is_finite()
        && value >= minimum
        && value <= maximum
        && (!maximum_exclusive || value < maximum))
        .then_some(value)
}

/// Aggregate linear or circular metric values using source arithmetic order.
fn aggregate_values(metric: Metric, values: &[f64]) -> Option<f64> {
    // preserve gust maxima and circular bearing means
    match metric {
        Metric::WindGust => values.iter().copied().reduce(f64::max),
        Metric::WindDirection => mean_direction(values),
        _ => mean(values),
    }
}

/// Divide each contributor before summing to match the source overflow behavior.
fn mean(values: &[f64]) -> Option<f64> {
    (!values.is_empty()).then(|| {
        values
            .iter()
            .fold(0.0, |total, value| total + value / values.len() as f64)
    })
}

/// Average bearings around north without inventing an opposing direction.
fn mean_direction(values: &[f64]) -> Option<f64> {
    let east = mean(
        &values
            .iter()
            .map(|value| (value * PI / 180.0).sin())
            .collect::<Vec<_>>(),
    )?;
    let north = mean(
        &values
            .iter()
            .map(|value| (value * PI / 180.0).cos())
            .collect::<Vec<_>>(),
    )?;
    // reject a directionless opposing vector
    if east.hypot(north) < 0.000_001 {
        None
    } else {
        Some((east.atan2(north) * 180.0 / PI + 360.0) % 360.0)
    }
}

#[derive(Clone)]
struct SolarSample<'a> {
    provider_key: &'static str,
    record: &'a ForecastRecordedInputRecord,
    source_key: &'static str,
    source_kind: &'static str,
    station_slug: &'static str,
    value: f64,
}

impl SolarSample<'_> {
    /// Return the original observation instant.
    const fn valid_ms(&self) -> i64 {
        self.record.valid_at.epoch_ms
    }

    /// Return the original reception instant.
    const fn received_ms(&self) -> i64 {
        self.record.received_at.epoch_ms
    }

    /// Encode the exact copied scalar evidence contract.
    fn value(&self) -> JsonValue {
        object(vec![
            ("providerKey", text(self.provider_key)),
            ("sourceKind", text(self.source_kind)),
            ("sourceKey", text(self.source_key)),
            ("stationSlug", text(self.station_slug)),
            ("value", JsonValue::Number(self.value)),
            ("validAt", text(&self.record.valid_at.value)),
            ("receivedAt", text(&self.record.received_at.value)),
            ("recordId", text(&self.record.id)),
            ("sourceId", text(&self.record.provenance.source_id)),
        ])
    }
}

/// Select one source-compatible sample for the requested evidence class.
fn solar_sample(record: &ForecastRecordedInputRecord, ws90: bool) -> Option<SolarSample<'_>> {
    let (provider_key, source_kind, source_key, station_slug, value) = if ws90 {
        (
            "ecowitt-local",
            ForecastRecordedInputSourceKind::PhysicalSensor,
            "ecowitt-88f15505d89f-local-live-v1",
            "ballydidean-ecowitt",
            record.metrics.solar_radiation_wm2?,
        )
    } else {
        (
            "open-meteo",
            ForecastRecordedInputSourceKind::ModelCurrent,
            "open-meteo-current-v1",
            "open-meteo-virtual",
            record.metrics.cloud_cover_percent?,
        )
    };
    // require the exact source identity and bounded copied strings
    if record.provenance.provider_key != provider_key
        || record.provenance.source_kind != source_kind
        || record.provenance.source_key != source_key
        || record.provenance.station_slug != station_slug
        || record.id.trim().is_empty()
        || record.id.len() > 512
        || record.provenance.source_id.trim().is_empty()
        || record.provenance.source_id.len() > 512
        || !value.is_finite()
    {
        return None;
    }
    Some(SolarSample {
        provider_key,
        record,
        source_key,
        source_kind: if ws90 {
            "physical_sensor"
        } else {
            "model_current"
        },
        station_slug,
        value,
    })
}

/// Port the bounded newest-daylight-pair search.
fn solar_cloud_evidence(
    records: &[ForecastRecordedInputRecord],
    site: ForecastRecordedSolarSite,
    through: &AdmittedForecastRecordedInstant,
) -> Result<Option<JsonValue>, NativeForecastRecordedProjectionError> {
    let now_ms = through.epoch_ms;
    // refuse unbounded history before pairing
    if records.len() > 20_000 {
        return Err(NativeForecastRecordedProjectionError::SolarHistoryLimit);
    }
    let mut ws90 = eligible_solar_samples(records, true, now_ms);
    let mut regional = eligible_solar_samples(records, false, now_ms);
    ws90.sort_by(|left, right| {
        right
            .valid_ms()
            .cmp(&left.valid_ms())
            .then_with(|| right.received_ms().cmp(&left.received_ms()))
            .then_with(|| left.record.id.cmp(&right.record.id))
    });
    regional.sort_by(|left, right| {
        left.valid_ms()
            .cmp(&right.valid_ms())
            .then_with(|| right.received_ms().cmp(&left.received_ms()))
            .then_with(|| left.record.id.cmp(&right.record.id))
    });
    let mut comparisons = 0usize;
    // stop at the newest ws90 observation with any viable regional pair
    for observed in &ws90 {
        let left_edge = observed.valid_ms().saturating_sub(PAIR_GAP_MS);
        let right_edge = observed.valid_ms().saturating_add(PAIR_GAP_MS);
        let mut lower = 0usize;
        let mut upper = regional.len();
        // find the first regional sample inside the left edge
        while lower < upper {
            let middle = (lower + upper) / 2;
            // skip samples strictly before the pairing window
            if regional[middle].valid_ms() < left_edge {
                lower = middle + 1;
            } else {
                upper = middle;
            }
        }
        let mut best: Option<&SolarSample<'_>> = None;
        // inspect only the bounded nearby regional interval
        for baseline in regional[lower..]
            .iter()
            .take_while(|sample| sample.valid_ms() <= right_edge)
        {
            comparisons = comparisons.saturating_add(1);
            // fail rather than publish an incomplete newest-pair search
            if comparisons > MAX_PAIR_COMPARISONS {
                return Err(NativeForecastRecordedProjectionError::SolarPairingLimit);
            }
            // require the browser estimator's exact daylight and age gates
            if !valid_solar_pair(observed, baseline, site, now_ms) {
                continue;
            }
            let evaluated = observed.received_ms().max(baseline.received_ms());
            // prefer the newest evaluation then latest regional original time
            let replace = match best {
                None => true,
                Some(current) => {
                    let current_evaluated = observed.received_ms().max(current.received_ms());
                    evaluated > current_evaluated
                        || evaluated == current_evaluated
                            && baseline.valid_ms() > current.valid_ms()
                }
            };
            // retain the newest evaluation then latest regional original time
            if replace {
                best = Some(baseline);
            }
        }
        // return after exhausting every revision at the newest viable observation
        if let Some(baseline) = best {
            let evaluated = observed.received_ms().max(baseline.received_ms());
            let evaluated_at = NativeWirePrimitives
                .format_utc_milliseconds(evaluated)
                .ok_or(NativeForecastRecordedProjectionError::Time)?;
            return Ok(Some(object(vec![
                ("evaluatedAt", text(&evaluated_at)),
                ("ws90", observed.value()),
                ("regional", baseline.value()),
            ])));
        }
    }
    Ok(None)
}

/// Retain only published samples inside the still-live evidence horizon.
fn eligible_solar_samples(
    records: &[ForecastRecordedInputRecord],
    ws90: bool,
    now_ms: i64,
) -> Vec<SolarSample<'_>> {
    records
        .iter()
        .filter_map(|record| solar_sample(record, ws90))
        .filter(|sample| {
            sample.valid_ms() < now_ms
                && sample.received_ms() <= now_ms
                && sample.received_ms() >= sample.valid_ms()
                && sample.valid_ms() >= now_ms.saturating_sub(DAY_MS + PAIR_GAP_MS)
        })
        .collect()
}

/// Reproduce the evidence rehydration and bias-construction validity gates.
fn valid_solar_pair(
    observed: &SolarSample<'_>,
    regional: &SolarSample<'_>,
    site: ForecastRecordedSolarSite,
    now_ms: i64,
) -> bool {
    let evaluated = observed.received_ms().max(regional.received_ms());
    // bind evaluation to real receptions and the live one-day horizon
    if evaluated > now_ms || observed.valid_ms().saturating_add(DAY_MS) <= now_ms {
        return false;
    }
    // preserve the estimator's causal freshness and pair-gap limits
    if observed.valid_ms() > evaluated
        || observed.received_ms() > evaluated
        || regional.valid_ms() > evaluated
        || regional.received_ms() > evaluated
        || observed.received_ms() < observed.valid_ms()
        || regional.received_ms() < regional.valid_ms()
        || evaluated.saturating_sub(observed.valid_ms()) > MAX_WS90_AGE_MS
        || evaluated.saturating_sub(observed.received_ms()) > MAX_WS90_AGE_MS
        || evaluated.saturating_sub(regional.valid_ms()) > MAX_REGIONAL_AGE_MS
        || evaluated.saturating_sub(regional.received_ms()) > MAX_REGIONAL_AGE_MS
        || observed.valid_ms().abs_diff(regional.valid_ms()) > PAIR_GAP_MS as u64
    {
        return false;
    }
    // require bounded physical and regional scalar inputs
    if observed.value <= 5.0
        || observed.value > 2_500.0
        || regional.value < 0.0
        || regional.value > 100.0
    {
        return false;
    }
    let Some(observed_sky) = clear_sky(site, observed.valid_ms()) else {
        return false;
    };
    let Some(evaluated_sky) = clear_sky(site, evaluated) else {
        return false;
    };
    // reject night, low sun, and weak expected radiation
    if observed_sky.elevation < MIN_SOLAR_ELEVATION_DEGREES
        || evaluated_sky.elevation < MIN_SOLAR_ELEVATION_DEGREES
        || observed_sky.irradiance < MIN_CLEAR_SKY_RADIATION_WM2
        || evaluated_sky.irradiance < MIN_CLEAR_SKY_RADIATION_WM2
        || observed.value / observed_sky.irradiance > MAX_RADIATION_RATIO
    {
        return false;
    }
    let obscuration = ((1.0
        - (observed.value / (CLEAR_SKY_TOLERANCE * observed_sky.irradiance)).min(1.0))
        / CLOUD_ATTENUATION_RANGE)
        .clamp(0.0, 1.0);
    let estimated_cloud = 100.0 * obscuration.sqrt();
    estimated_cloud.is_finite()
}

struct ClearSky {
    elevation: f64,
    irradiance: f64,
}

/// Calculate NOAA solar geometry and Haurwitz clear-sky irradiance.
fn clear_sky(site: ForecastRecordedSolarSite, epoch_ms: i64) -> Option<ClearSky> {
    // reject invalid coordinates before trigonometry
    if !site.latitude.is_finite()
        || !(-90.0..=90.0).contains(&site.latitude)
        || !site.longitude.is_finite()
        || !(-180.0..=180.0).contains(&site.longitude)
    {
        return None;
    }
    let julian_day = epoch_ms as f64 / DAY_MS as f64 + 2_440_587.5;
    let julian_century = (julian_day - 2_451_545.0) / 36_525.0;
    let geometric_mean_longitude = normalize(
        280.46646 + julian_century * (36_000.769_83 + julian_century * 0.0003032),
        360.0,
    );
    let geometric_mean_anomaly =
        357.52911 + julian_century * (35_999.050_29 - 0.0001537 * julian_century);
    let orbital_eccentricity =
        0.016708634 - julian_century * (0.000042037 + 0.0000001267 * julian_century);
    let anomaly = geometric_mean_anomaly.to_radians();
    let equation_of_center = anomaly.sin()
        * (1.914602 - julian_century * (0.004817 + 0.000014 * julian_century))
        + (2.0 * anomaly).sin() * (0.019993 - 0.000101 * julian_century)
        + (3.0 * anomaly).sin() * 0.000289;
    let true_longitude = geometric_mean_longitude + equation_of_center;
    let apparent_longitude = true_longitude
        - 0.00569
        - 0.00478 * (125.04 - 1934.136 * julian_century).to_radians().sin();
    let mean_obliquity = 23.0
        + (26.0
            + (21.448
                - julian_century
                    * (46.815 + julian_century * (0.00059 - julian_century * 0.001813)))
                / 60.0)
            / 60.0;
    let corrected_obliquity =
        mean_obliquity + 0.00256 * (125.04 - 1934.136 * julian_century).to_radians().cos();
    let obliquity = corrected_obliquity.to_radians();
    let apparent = apparent_longitude.to_radians();
    let solar_declination = (obliquity.sin() * apparent.sin()).asin();
    let equation_variable = (obliquity / 2.0).tan().powi(2);
    let longitude = geometric_mean_longitude.to_radians();
    let equation_of_time = 4.0
        * (equation_variable * (2.0 * longitude).sin()
            - 2.0 * orbital_eccentricity * anomaly.sin()
            + 4.0
                * orbital_eccentricity
                * equation_variable
                * anomaly.sin()
                * (2.0 * longitude).cos()
            - 0.5 * equation_variable.powi(2) * (4.0 * longitude).sin()
            - 1.25 * orbital_eccentricity.powi(2) * (2.0 * anomaly).sin())
        .to_degrees();
    let utc_minutes = epoch_ms.rem_euclid(DAY_MS) as f64 / 60_000.0;
    let true_solar_minutes = normalize(
        utc_minutes + equation_of_time + 4.0 * site.longitude,
        1440.0,
    );
    let hour_angle = (true_solar_minutes / 4.0 - 180.0).to_radians();
    let latitude = site.latitude.to_radians();
    let cosine_zenith = (latitude.sin() * solar_declination.sin()
        + latitude.cos() * solar_declination.cos() * hour_angle.cos())
    .clamp(-1.0, 1.0);
    let elevation = 90.0 - cosine_zenith.acos().to_degrees();
    // retain zero irradiance below the horizon
    if cosine_zenith <= 0.0 {
        return Some(ClearSky {
            elevation,
            irradiance: 0.0,
        });
    }
    Some(ClearSky {
        elevation,
        irradiance: 1098.0 * cosine_zenith * (-0.059 / cosine_zenith).exp(),
    })
}

/// Normalize one value into a positive modular interval.
fn normalize(value: f64, modulus: f64) -> f64 {
    ((value % modulus) + modulus) % modulus
}

/// Build one producer-ordered generated JSON object.
fn object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(name, value)| (name.encode_utf16().collect(), value))
            .collect(),
    )
}

/// Build one generated JSON string.
fn text(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use weather_edge::{
        ForecastRecordedInputMetrics, ForecastRecordedInputProvenance,
        ForecastRecordedInputSourceKind, RevisionArchiveCanonicalCodec,
    };

    /// Construct one canonical test instant.
    fn instant(value: &str) -> AdmittedForecastRecordedInstant {
        let epoch_ms = NativeWirePrimitives
            .canonical_utc_milliseconds(value)
            .unwrap();
        AdmittedForecastRecordedInstant::admitted(value, epoch_ms)
    }

    /// Construct one sparse typed input record.
    fn record(
        id: &str,
        source: (ForecastRecordedInputSourceKind, &str, &str, &str),
        clocks: (&str, &str),
        metrics: (Option<f64>, Option<f64>),
    ) -> ForecastRecordedInputRecord {
        let (source_kind, provider, source_key, station) = source;
        let (valid_at, received_at) = clocks;
        let (radiation, cloud) = metrics;
        ForecastRecordedInputRecord {
            id: id.to_owned(),
            metrics: ForecastRecordedInputMetrics {
                apparent_temperature_c: None,
                cloud_cover_percent: cloud,
                pm25_micrograms_per_cubic_meter: None,
                precipitation_rate_mm_per_hour: None,
                pressure_hpa: None,
                relative_humidity_percent: None,
                solar_radiation_wm2: radiation,
                temperature_c: None,
                uv_index: None,
                wind_direction_degrees: None,
                wind_gust_mps: None,
                wind_speed_mps: None,
            },
            provenance: ForecastRecordedInputProvenance {
                provider_key: provider.to_owned(),
                source_id: format!("source-{id}"),
                source_key: source_key.to_owned(),
                source_kind,
                station_slug: station.to_owned(),
            },
            received_at: instant(received_at),
            valid_at: instant(valid_at),
        }
    }

    /// Verify an empty complete hour retains the exact public point shape.
    #[test]
    fn projects_empty_hour() {
        let projection = NativeForecastRecordedProjectionOwner;
        let from = instant("2026-11-24T08:00:00.000Z");
        let through = instant("2026-11-24T09:00:00.000Z");
        let JsonValue::Array(points) = projection
            .forecast_observation_points(&[], &from, &through)
            .unwrap()
        else {
            panic!("expected points");
        };
        assert_eq!(points.len(), 1);
        let encoded = NativeWirePrimitives
            .encode_ordered_json(&points[0])
            .unwrap();
        assert_eq!(
            String::from_utf8(encoded).unwrap(),
            "{\"validAt\":\"2026-11-24T08:00:00.000Z\",\"metrics\":{},\"pressureChange3hHpa\":null,\"sources\":{},\"counts\":{},\"recordedThrough\":\"2026-11-24T09:00:00.000Z\",\"partial\":false}\n"
        );
    }

    /// Verify a real daylight physical/regional pair yields copied evidence.
    #[test]
    fn projects_solar_cloud_pair() {
        let records = vec![
            record(
                "1",
                (
                    ForecastRecordedInputSourceKind::PhysicalSensor,
                    "ecowitt-local",
                    "ecowitt-88f15505d89f-local-live-v1",
                    "ballydidean-ecowitt",
                ),
                ("2026-06-21T19:58:00.000Z", "2026-06-21T19:59:00.000Z"),
                (Some(800.0), None),
            ),
            record(
                "2",
                (
                    ForecastRecordedInputSourceKind::ModelCurrent,
                    "open-meteo",
                    "open-meteo-current-v1",
                    "open-meteo-virtual",
                ),
                ("2026-06-21T20:00:00.000Z", "2026-06-21T20:01:00.000Z"),
                (None, Some(30.0)),
            ),
        ];
        let evidence = NativeForecastRecordedProjectionOwner
            .solar_cloud_evidence(
                &records,
                ForecastRecordedSolarSite {
                    latitude: 47.950_429_954_185_445,
                    longitude: -122.427_970_126_081_93,
                },
                &instant("2026-06-21T20:05:00.000Z"),
            )
            .unwrap()
            .unwrap();
        let encoded = NativeWirePrimitives.encode_ordered_json(&evidence).unwrap();
        assert!(
            String::from_utf8(encoded)
                .unwrap()
                .starts_with("{\"evaluatedAt\":\"2026-06-21T20:01:00.000Z\",\"ws90\":")
        );
    }
}
