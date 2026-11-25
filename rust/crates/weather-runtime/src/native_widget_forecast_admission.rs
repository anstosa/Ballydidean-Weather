//! Source-backed forecast admission matching the browser's fail-raw family boundaries.

use weather_contracts::JsonValue;
use weather_edge::ArchiveWirePrimitives;

use crate::{NativeWirePrimitives, maintenance_canonical_json::field};

const HOUR_MS: i64 = 3_600_000;
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

const RAW_METRIC_KEYS: &[&str] = &[
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
    "windDirectionDegrees",
    "windGustMps",
    "windSpeedMps",
    "wetBulbGlobeTemperatureC",
];
const GENERIC_RUNTIME_KEYS: &[&str] = &[
    "actionSha256",
    "activationMode",
    "activeBundle",
    "authorizationSha256",
    "candidateArtifactSha256",
    "enabledMetrics",
    "evaluationReportSha256",
    "expiresAt",
    "fullMemberRootSha256",
    "loadedAt",
    "qualificationReceiptSha256",
    "reasonCode",
    "state",
    "policyReportSha256",
    "transferReportSha256",
];
const GENERIC_RUNTIME_LEGACY_KEYS: &[&str] = &[
    "activationMode",
    "activeBundle",
    "authorizationSha256",
    "candidateArtifactSha256",
    "enabledMetrics",
    "evaluationReportSha256",
    "expiresAt",
    "loadedAt",
    "qualificationReceiptSha256",
    "reasonCode",
    "state",
    "transferReportSha256",
];
const GENERIC_FAIL_RAW_KEYS: &[&str] = &[
    "adjustedMetrics",
    "appliedMetrics",
    "contractVersion",
    "reasonCode",
    "state",
];
const GENERIC_ACTIVE_KEYS: &[&str] = &[
    "adjustedMetrics",
    "algorithmContractVersion",
    "appliedMetrics",
    "candidateArtifactSha256",
    "contractVersion",
    "evaluationReportSha256",
    "leadBand",
    "qualificationReceiptSha256",
    "rawForecastProvenance",
    "reasonCode",
    "state",
];
const GENERIC_WIND_ACTIVE_KEYS: &[&str] = &[
    "activationKind",
    "adjustedMetrics",
    "algorithmContractVersion",
    "appliedMetrics",
    "authorizationSha256",
    "candidateArtifactSha256",
    "contractVersion",
    "leadBand",
    "rawForecastProvenance",
    "reasonCode",
    "state",
    "transferReportSha256",
];
const GENERIC_MAINTENANCE_ACTIVE_KEYS: &[&str] = &[
    "actionSha256",
    "activationKind",
    "adjustedMetrics",
    "algorithmContractVersion",
    "appliedMetrics",
    "candidateArtifactSha256",
    "contractVersion",
    "fullMemberRootSha256",
    "leadBand",
    "policyReportSha256",
    "rawForecastProvenance",
    "reasonCode",
    "state",
];
const GENERIC_PROVENANCE_KEYS: &[&str] = &[
    "adapterVersion",
    "cohort",
    "contractEpoch",
    "dataset",
    "referenceAt",
    "referenceKind",
    "sourceConfigFingerprint",
    "sourceKey",
    "targetLeadHours",
    "upstreamModel",
    "validAt",
];
const TEMPERATURE_RUNTIME_KEYS: &[&str] = &[
    "actionSha256",
    "activationMode",
    "activeBundle",
    "authorizationSha256",
    "expiresAt",
    "fullMemberRootSha256",
    "loadedAt",
    "policyReportSha256",
    "reasonCode",
    "source",
    "state",
];
const TEMPERATURE_RUNTIME_LEGACY_KEYS: &[&str] = &[
    "activeBundle",
    "authorizationSha256",
    "expiresAt",
    "loadedAt",
    "reasonCode",
    "source",
    "state",
];
const TEMPERATURE_RUNTIME_SOURCE_KEYS: &[&str] = &[
    "adaptiveReady",
    "firstReceivedAt",
    "hourCount",
    "latestRunInitializedAt",
    "stateReason",
    "stateStatus",
];
const TEMPERATURE_DECISION_KEYS: &[&str] = &[
    "branch",
    "bundleSha256",
    "contractVersion",
    "correctedTemperatureC",
    "rawBestMatchTemperatureC",
    "reasonCode",
    "recentErrorStateSha256",
    "sourceForecast",
    "state",
];
const TEMPERATURE_SOURCE_KEYS: &[&str] = &[
    "adapterVersion",
    "dataset",
    "firstReceivedAt",
    "modelCycle",
    "modelLeadHours",
    "operationalHorizonHours",
    "providerKey",
    "providerResponseSha256",
    "rawRelativeHumidityPercent",
    "rawTemperatureC",
    "rawWindSpeedMps",
    "runInitializedAt",
    "upstreamModel",
    "validAt",
];
const RAIN_RUNTIME_KEYS: &[&str] = &["state", "activeBundle", "reasonCode", "loadedAt", "source"];
const RAIN_RUNTIME_SOURCE_KEYS: &[&str] = &[
    "runInitializedAt",
    "firstReceivedAt",
    "decisionAt",
    "hourCount",
];
const RAIN_DECISION_KEYS: &[&str] = &[
    "contractVersion",
    "state",
    "reasonCode",
    "bundleSha256",
    "correctedPrecipitationMm",
    "rawBestMatchPrecipitationMm",
    "sourceForecast",
];
const RAIN_SOURCE_KEYS: &[&str] = &[
    "runInitializedAt",
    "firstReceivedAt",
    "validAt",
    "rawPrecipitationMm",
    "modelLeadHours",
    "decisionAt",
    "upstreamModel",
    "providerKey",
];
const SETTINGS_KEYS: &[&str] = &["version", "temperature", "wind", "rain"];

/// Closed forecast admission failures surfaced as the existing widget upstream failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NativeWidgetForecastAdmissionError {
    Response,
    AirQualityContext,
    RawRecord,
}

/// One browser-equivalent safe response with malformed adjustment families failed raw.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AdmittedNativeWidgetForecast {
    pub(crate) response: JsonValue,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GenericMetric {
    RelativeHumidity,
    Temperature,
    WindDirection,
    WindGust,
    WindSpeed,
}

impl GenericMetric {
    /// Decode one governed generic metric name.
    fn parse(value: &str) -> Option<Self> {
        match value {
            "relativeHumidityPercent" => Some(Self::RelativeHumidity),
            "temperatureC" => Some(Self::Temperature),
            "windDirectionDegrees" => Some(Self::WindDirection),
            "windGustMps" => Some(Self::WindGust),
            "windSpeedMps" => Some(Self::WindSpeed),
            _ => None,
        }
    }

    /// Return the canonical JSON property name for one governed metric.
    const fn name(self) -> &'static str {
        match self {
            Self::RelativeHumidity => "relativeHumidityPercent",
            Self::Temperature => "temperatureC",
            Self::WindDirection => "windDirectionDegrees",
            Self::WindGust => "windGustMps",
            Self::WindSpeed => "windSpeedMps",
        }
    }

    /// Report whether one selected metric belongs to the wind-only activation family.
    const fn is_wind(self) -> bool {
        matches!(self, Self::WindDirection | Self::WindGust | Self::WindSpeed)
    }

    /// Enforce the exact browser adjustment bounds for one selected value.
    fn admits(self, value: f64) -> bool {
        value.is_finite()
            && match self {
                Self::RelativeHumidity => (0.0..=100.0).contains(&value),
                Self::Temperature => (-100.0..=70.0).contains(&value),
                Self::WindDirection => (0.0..360.0).contains(&value),
                Self::WindGust | Self::WindSpeed => (0.0..=150.0).contains(&value),
            }
    }
}

#[derive(Clone)]
struct RawRecord {
    value: JsonValue,
    valid_at: String,
    valid_at_epoch_ms: i64,
    product_run_at: Option<String>,
    source_id: String,
    source_key: String,
    source_kind: String,
    provider_dataset: Option<String>,
    upstream_model: Option<String>,
    metrics: RawMetrics,
}

#[derive(Clone, Copy)]
struct RawMetrics {
    relative_humidity_percent: Option<f64>,
    temperature_c: Option<f64>,
    wind_direction_degrees: Option<f64>,
    wind_gust_mps: Option<f64>,
    wind_speed_mps: Option<f64>,
    precipitation_mm: Option<f64>,
    pressure_hpa: Option<f64>,
}

impl RawMetrics {
    /// Select one governed metric from a validated raw record.
    const fn generic(self, metric: GenericMetric) -> Option<f64> {
        match metric {
            GenericMetric::RelativeHumidity => self.relative_humidity_percent,
            GenericMetric::Temperature => self.temperature_c,
            GenericMetric::WindDirection => self.wind_direction_degrees,
            GenericMetric::WindGust => self.wind_gust_mps,
            GenericMetric::WindSpeed => self.wind_speed_mps,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RuntimeState {
    Active,
    Disabled,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum GenericActivation {
    Qualified,
    WindCanary,
    Maintenance,
}

#[derive(Clone)]
struct GenericRuntime {
    value: JsonValue,
    state: RuntimeState,
    activation: Option<GenericActivation>,
    authorization_sha256: Option<String>,
    candidate_sha256: Option<String>,
    enabled_metrics: Vec<GenericMetric>,
    evaluation_sha256: Option<String>,
    qualification_sha256: Option<String>,
    transfer_sha256: Option<String>,
    action_sha256: Option<String>,
    full_member_root_sha256: Option<String>,
    policy_report_sha256: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TemperatureActivation {
    Canary,
    Maintenance,
}

#[derive(Clone)]
struct TemperatureRuntime {
    value: JsonValue,
    state: RuntimeState,
    active_bundle: Option<String>,
}

#[derive(Clone)]
struct RainRuntime {
    value: JsonValue,
    state: RuntimeState,
    active_bundle: Option<String>,
}

#[derive(Clone, Copy)]
struct AdjustmentSettings {
    temperature: bool,
    wind: bool,
    rain: bool,
}

impl AdjustmentSettings {
    /// Return the fail-closed settings used when the property is present but malformed.
    const fn disabled() -> Self {
        Self {
            temperature: false,
            wind: false,
            rain: false,
        }
    }

    /// Select whether one generic adjusted metric remains visible.
    const fn admits_metric(self, metric: GenericMetric) -> bool {
        match metric {
            GenericMetric::RelativeHumidity => self.temperature || self.wind || self.rain,
            GenericMetric::Temperature => self.temperature,
            GenericMetric::WindDirection | GenericMetric::WindGust | GenericMetric::WindSpeed => {
                self.wind
            }
        }
    }
}

/// Admit one forecast response and reproduce the browser's all-row family fallback boundary.
pub(crate) fn admit_native_widget_forecast(
    value: &JsonValue,
) -> Result<AdmittedNativeWidgetForecast, NativeWidgetForecastAdmissionError> {
    let JsonValue::Object(_) = value else {
        return Err(NativeWidgetForecastAdmissionError::Response);
    };
    let JsonValue::Array(visible) =
        field(value, "data").ok_or(NativeWidgetForecastAdmissionError::Response)?
    else {
        return Err(NativeWidgetForecastAdmissionError::Response);
    };
    // match the browser's ordinary response boundary before adjustment fallback
    if !matches!(field(value, "site"), Some(JsonValue::Object(_))) {
        return Err(NativeWidgetForecastAdmissionError::Response);
    }
    let context = match field(value, "airQualityContext") {
        None => &[][..],
        Some(JsonValue::Array(rows)) if rows.len() <= 64 => rows.as_slice(),
        Some(_) => return Err(NativeWidgetForecastAdmissionError::AirQualityContext),
    };
    let visible_count = visible.len();
    let mut source_rows = Vec::with_capacity(visible_count + context.len());
    source_rows.extend(visible.iter());
    source_rows.extend(context.iter());
    let mut raw_records = Vec::with_capacity(source_rows.len());
    // reject a malformed raw row before evaluating any adjustment family
    for row in &source_rows {
        raw_records
            .push(parse_raw_record(row).ok_or(NativeWidgetForecastAdmissionError::RawRecord)?);
    }

    let generic_runtime = field(value, "adjustmentRuntime").and_then(parse_generic_runtime);
    let generic_decisions = generic_runtime
        .as_ref()
        .and_then(|runtime| parse_complete_generic_family(&source_rows, &raw_records, runtime));
    let generic_valid = generic_runtime.is_some() && generic_decisions.is_some();
    let mut generic_rows = raw_records
        .iter()
        .map(|record| record.value.clone())
        .collect::<Vec<_>>();
    // publish generic metadata only after every visible and context row validates
    if let Some(decisions) = generic_decisions.as_ref() {
        for (row, decision) in generic_rows.iter_mut().zip(decisions) {
            set_object_field(row, "adjustment", decision.clone());
        }
    }

    let temperature_runtime =
        field(value, "temperatureAdjustmentRuntime").and_then(parse_temperature_runtime);
    let temperature_decisions = temperature_runtime
        .as_ref()
        .and_then(|runtime| parse_complete_temperature_family(&source_rows, &raw_records, runtime));
    let temperature_valid = temperature_runtime.is_some() && temperature_decisions.is_some();
    let mut temperature_rows = generic_rows.clone();
    // publish independent temperature metadata only after the complete family validates
    if let Some(decisions) = temperature_decisions.as_ref() {
        for (row, decision) in temperature_rows.iter_mut().zip(decisions) {
            set_object_field(row, "temperatureAdjustment", decision.clone());
        }
    }

    let rain_runtime = field(value, "rainAdjustmentRuntime").and_then(parse_rain_runtime);
    let rain_decisions = rain_runtime
        .as_ref()
        .and_then(|runtime| parse_complete_rain_family(&source_rows, &raw_records, runtime));
    let rain_valid = rain_runtime.is_some() && rain_decisions.is_some();
    let rain_base = if temperature_runtime.is_some() && !temperature_valid {
        &generic_rows
    } else {
        &temperature_rows
    };
    let mut rain_rows = rain_base.clone();
    // publish independent rain metadata only after the complete family validates
    if let Some(decisions) = rain_decisions.as_ref() {
        for (row, decision) in rain_rows.iter_mut().zip(decisions) {
            set_object_field(row, "rainAdjustment", decision.clone());
        }
    }

    let settings_present = field(value, "adjustmentSettings").is_some();
    let settings = field(value, "adjustmentSettings")
        .and_then(parse_settings)
        .or_else(|| settings_present.then_some(AdjustmentSettings::disabled()));
    let selected_base = if rain_runtime.is_some() && !rain_valid {
        rain_base.to_vec()
    } else {
        rain_rows
    };
    let safe_rows = selected_base
        .into_iter()
        .map(|row| project_settings(row, settings))
        .collect::<Vec<_>>();

    let mut response = value.clone();
    set_object_field(
        &mut response,
        "data",
        JsonValue::Array(safe_rows[..visible_count].to_vec()),
    );
    set_object_field(
        &mut response,
        "airQualityContext",
        JsonValue::Array(safe_rows[visible_count..].to_vec()),
    );
    set_object_field(
        &mut response,
        "adjustmentRuntime",
        if generic_valid {
            generic_runtime.expect("validated generic runtime").value
        } else {
            invalid_generic_runtime()
        },
    );
    set_object_field(
        &mut response,
        "temperatureAdjustmentRuntime",
        if temperature_valid {
            temperature_runtime
                .expect("validated temperature runtime")
                .value
        } else {
            invalid_temperature_runtime()
        },
    );
    set_object_field(
        &mut response,
        "rainAdjustmentRuntime",
        if rain_valid {
            rain_runtime.expect("validated rain runtime").value
        } else {
            invalid_rain_runtime()
        },
    );
    set_object_field(
        &mut response,
        "adjustmentSettings",
        settings.map_or(JsonValue::Null, settings_value),
    );
    set_object_field(
        &mut response,
        "pressureContext",
        JsonValue::Array(parse_pressure_context(field(value, "pressureContext"))),
    );
    Ok(AdmittedNativeWidgetForecast { response })
}

/// Parse one complete retained pressure window without mixing forecast vintages.
fn parse_pressure_context(value: Option<&JsonValue>) -> Vec<JsonValue> {
    let Some(JsonValue::Array(rows)) = value else {
        return Vec::new();
    };
    // keep an incomplete pressure context independently harmless
    if rows.len() != 6 {
        return Vec::new();
    }
    let mut records = Vec::with_capacity(rows.len());
    // require six complete forecast pressure endpoints
    for row in rows {
        let Some(record) = parse_raw_record(row) else {
            return Vec::new();
        };
        if record.source_kind != "forecast"
            || record.product_run_at.is_none()
            || record.metrics.pressure_hpa.is_none()
        {
            return Vec::new();
        }
        records.push(record);
    }
    records.sort_by_key(|record| record.valid_at_epoch_ms);
    let first = &records[0];
    // retain only one exact contiguous product run and source
    if !records.iter().enumerate().all(|(index, record)| {
        record.source_id == first.source_id
            && record.product_run_at == first.product_run_at
            && record.valid_at_epoch_ms
                == first.valid_at_epoch_ms + i64::try_from(index).unwrap_or(i64::MAX) * HOUR_MS
    }) {
        return Vec::new();
    }
    records.into_iter().map(|record| record.value).collect()
}

/// Validate one complete generic decision family without exposing partial success.
fn parse_complete_generic_family(
    source_rows: &[&JsonValue],
    records: &[RawRecord],
    runtime: &GenericRuntime,
) -> Option<Vec<JsonValue>> {
    let mut decisions = Vec::with_capacity(records.len());
    // fail the whole family when any visible or context row is missing or inconsistent
    for (source, record) in source_rows.iter().zip(records) {
        let decision = parse_generic_decision(field(source, "adjustment")?, record, runtime)?;
        // retain the browser's disabled-runtime cross-family guard
        if runtime.state == RuntimeState::Disabled && text_field(&decision, "state")? == "active" {
            return None;
        }
        decisions.push(decision);
    }
    Some(decisions)
}

/// Validate one complete temperature decision family without exposing partial success.
fn parse_complete_temperature_family(
    source_rows: &[&JsonValue],
    records: &[RawRecord],
    runtime: &TemperatureRuntime,
) -> Option<Vec<JsonValue>> {
    let mut decisions = Vec::with_capacity(records.len());
    // fail the whole family when any visible or context row is missing or inconsistent
    for (source, record) in source_rows.iter().zip(records) {
        decisions.push(parse_temperature_decision(
            field(source, "temperatureAdjustment")?,
            record,
            runtime,
        )?);
    }
    Some(decisions)
}

/// Validate one complete rain decision family without exposing partial success.
fn parse_complete_rain_family(
    source_rows: &[&JsonValue],
    records: &[RawRecord],
    runtime: &RainRuntime,
) -> Option<Vec<JsonValue>> {
    let mut decisions = Vec::with_capacity(records.len());
    // fail the whole family when any visible or context row is missing or inconsistent
    for (source, record) in source_rows.iter().zip(records) {
        decisions.push(parse_rain_decision(
            field(source, "rainAdjustment")?,
            record,
            runtime,
        )?);
    }
    Some(decisions)
}

/// Validate and strip adjustment metadata from one complete raw weather record.
fn parse_raw_record(value: &JsonValue) -> Option<RawRecord> {
    let JsonValue::Object(_) = value else {
        return None;
    };
    let freshness = object_field(value, "freshness")?;
    let age_seconds = number_field(freshness, "ageSeconds")?;
    let freshness_status = text_field(freshness, "status")?;
    // require the complete safe freshness boundary
    if !is_safe_integer(age_seconds)
        || age_seconds < 0.0
        || text_field(freshness, "label").is_none()
        || !matches!(freshness_status.as_str(), "delayed" | "fresh" | "stale")
    {
        return None;
    }
    text_field(value, "id")?;
    let metadata = object_field(value, "metadata")?;
    let (provider_dataset, upstream_model) = validate_metadata(metadata)?;
    let metrics_value = object_field(value, "metrics")?;
    // require every canonical finite-or-null metric while allowing additive fields
    for key in RAW_METRIC_KEYS {
        nullable_number_field(metrics_value, key)?;
    }
    let product_run_at = nullable_text_field(value, "productRunAt")?;
    // require every non-null product run clock to carry an explicit valid offset
    if product_run_at
        .as_deref()
        .is_some_and(|instant| parse_instant(instant).is_none())
    {
        return None;
    }
    let provenance = object_field(value, "provenance")?;
    let (source_id, source_key, source_kind) = validate_provenance(provenance)?;
    let received_at = text_field(value, "receivedAt")?;
    let valid_at = text_field(value, "validAt")?;
    let valid_at_epoch_ms = parse_instant(&valid_at)?;
    let revision_count = number_field(value, "revisionCount")?;
    // reject malformed row clocks and counters through the same raw-record boundary
    if parse_instant(&received_at).is_none()
        || !is_safe_integer(revision_count)
        || revision_count < 0.0
    {
        return None;
    }
    let metrics = RawMetrics {
        relative_humidity_percent: nullable_number_field(metrics_value, "relativeHumidityPercent")?,
        temperature_c: nullable_number_field(metrics_value, "temperatureC")?,
        wind_direction_degrees: nullable_number_field(metrics_value, "windDirectionDegrees")?,
        wind_gust_mps: nullable_number_field(metrics_value, "windGustMps")?,
        wind_speed_mps: nullable_number_field(metrics_value, "windSpeedMps")?,
        precipitation_mm: nullable_number_field(metrics_value, "precipitationMm")?,
        pressure_hpa: nullable_number_field(metrics_value, "pressureHpa")?,
    };
    Some(RawRecord {
        value: object_without(
            value,
            &["adjustment", "rainAdjustment", "temperatureAdjustment"],
        )?,
        valid_at,
        valid_at_epoch_ms,
        product_run_at,
        source_id,
        source_key,
        source_kind,
        provider_dataset,
        upstream_model,
        metrics,
    })
}

/// Validate the complete public metadata shape and retain decision cross-link values.
fn validate_metadata(value: &JsonValue) -> Option<(Option<String>, Option<String>)> {
    validate_device(field(value, "device")?)?;
    let provider_dataset = validate_provider(field(value, "provider")?)?;
    validate_quality(field(value, "quality")?)?;
    let upstream = object_field(value, "upstream")?;
    let upstream_model = nullable_text_field(upstream, "model")?;
    text_field(upstream, "timezone")?;
    Some((provider_dataset, upstream_model))
}

/// Validate nullable device metadata without narrowing additive source fields.
fn validate_device(value: &JsonValue) -> Option<()> {
    // preserve a genuine null device branch
    if matches!(value, JsonValue::Null) {
        return Some(());
    }
    let device = as_object(value)?;
    nullable_text_field(device, "model")?;
    nullable_text_field(device, "serial")?;
    nullable_text_field(device, "vendor")?;
    Some(())
}

/// Validate nullable provider metadata and retain its dataset identity.
fn validate_provider(value: &JsonValue) -> Option<Option<String>> {
    // preserve a genuine null provider branch
    if matches!(value, JsonValue::Null) {
        return Some(None);
    }
    let provider = as_object(value)?;
    let dataset = nullable_text_field(provider, "dataset")?;
    nullable_number_field(provider, "elevationM")?;
    nullable_text_field(provider, "gridCell")?;
    validate_property_sensors(field(provider, "propertySensors")?)?;
    Some(dataset)
}

/// Validate nullable property sensor snapshots.
fn validate_property_sensors(value: &JsonValue) -> Option<()> {
    // preserve a genuine null property-sensor branch
    if matches!(value, JsonValue::Null) {
        return Some(());
    }
    let JsonValue::Array(sensors) = value else {
        return None;
    };
    // validate every additive sensor snapshot independently
    for sensor in sensors {
        let sensor = as_object(sensor)?;
        match field(sensor, "channel")? {
            JsonValue::Null => {}
            JsonValue::Number(channel)
                if is_safe_integer(*channel) && (1.0..=32.0).contains(channel) => {}
            _ => return None,
        }
        text_field(sensor, "key")?;
        text_field(sensor, "model")?;
        let readings = object_field(sensor, "readings")?;
        let JsonValue::Object(readings) = readings else {
            return None;
        };
        // require each additive property reading to remain finite
        for (_, reading) in readings {
            match reading {
                JsonValue::Number(value) if value.is_finite() => {}
                _ => return None,
            }
        }
    }
    Some(())
}

/// Validate nullable quality metadata without constraining additive flags.
fn validate_quality(value: &JsonValue) -> Option<()> {
    // preserve a genuine null quality branch
    if matches!(value, JsonValue::Null) {
        return Some(());
    }
    let quality = as_object(value)?;
    nullable_number_field(quality, "confidencePercent")?;
    match field(quality, "flags")? {
        JsonValue::Null => {}
        JsonValue::Array(flags) => {
            // require every quality flag to remain a string
            for flag in flags {
                string_value(flag)?;
            }
        }
        _ => return None,
    }
    nullable_text_field(quality, "interpolation")?;
    nullable_text_field(quality, "status")?;
    Some(())
}

/// Validate public provenance and retain its source-key identity.
fn validate_provenance(value: &JsonValue) -> Option<(String, String, String)> {
    let attribution = object_field(value, "attribution")?;
    text_field(attribution, "label")?;
    text_field(attribution, "url")?;
    text_field(value, "label")?;
    text_field(value, "providerKey")?;
    let source_id = text_field(value, "sourceId")?;
    let source_key = text_field(value, "sourceKey")?;
    let source_kind = text_field(value, "sourceKind")?;
    // admit only the canonical public source kinds
    if !matches!(
        source_kind.as_str(),
        "forecast"
            | "model_current"
            | "physical_sensor"
            | "reanalysis"
            | "tide_observation"
            | "tide_prediction"
    ) {
        return None;
    }
    text_field(value, "stationSlug")?;
    Some((source_id, source_key, source_kind))
}

/// Parse the response-level generic runtime including the legacy augmentation.
fn parse_generic_runtime(value: &JsonValue) -> Option<GenericRuntime> {
    let mut normalized = value.clone();
    // augment only the exact predecessor runtime shape
    if exact_keys(value, GENERIC_RUNTIME_LEGACY_KEYS) {
        set_object_field(&mut normalized, "actionSha256", JsonValue::Null);
        set_object_field(&mut normalized, "fullMemberRootSha256", JsonValue::Null);
        set_object_field(&mut normalized, "policyReportSha256", JsonValue::Null);
    }
    // reject every partial or extended runtime envelope
    if !exact_keys(&normalized, GENERIC_RUNTIME_KEYS) {
        return None;
    }
    parse_instant(&text_field(&normalized, "loadedAt")?)?;
    let enabled_values = array_field(&normalized, "enabledMetrics")?;
    let mut enabled_metrics = Vec::with_capacity(enabled_values.len());
    // validate and deduplicate every enabled metric
    for value in enabled_values {
        let metric = GenericMetric::parse(&string_value(value)?)?;
        if enabled_metrics.contains(&metric) {
            return None;
        }
        enabled_metrics.push(metric);
    }
    let state_text = text_field(&normalized, "state")?;
    // accept one complete active identity
    if state_text == "active" {
        let activation = match text_field(&normalized, "activationMode")?.as_str() {
            "qualified" => GenericActivation::Qualified,
            "wind_canary" => GenericActivation::WindCanary,
            "maintenance_qualified" => GenericActivation::Maintenance,
            _ => return None,
        };
        hash_field(&normalized, "activeBundle")?;
        let candidate_sha256 = hash_field(&normalized, "candidateArtifactSha256")?;
        let expires_at = nullable_text_field(&normalized, "expiresAt")?;
        // require active metrics, expiry validity, and wind-mode confinement
        if enabled_metrics.is_empty()
            || expires_at
                .as_deref()
                .is_some_and(|instant| parse_instant(instant).is_none())
            || (activation != GenericActivation::Qualified
                && !enabled_metrics.iter().all(|metric| metric.is_wind()))
            || !matches!(field(&normalized, "reasonCode"), Some(JsonValue::Null))
        {
            return None;
        }
        let authorization_sha256 = optional_hash_field(&normalized, "authorizationSha256")?;
        let evaluation_sha256 = optional_hash_field(&normalized, "evaluationReportSha256")?;
        let qualification_sha256 = optional_hash_field(&normalized, "qualificationReceiptSha256")?;
        let transfer_sha256 = optional_hash_field(&normalized, "transferReportSha256")?;
        let action_sha256 = optional_hash_field(&normalized, "actionSha256")?;
        let full_member_root_sha256 = optional_hash_field(&normalized, "fullMemberRootSha256")?;
        let policy_report_sha256 = optional_hash_field(&normalized, "policyReportSha256")?;
        let mode_identity_valid = match activation {
            GenericActivation::WindCanary => {
                authorization_sha256.is_some()
                    && transfer_sha256.is_some()
                    && evaluation_sha256.is_none()
                    && qualification_sha256.is_none()
                    && action_sha256.is_none()
                    && full_member_root_sha256.is_none()
                    && policy_report_sha256.is_none()
            }
            GenericActivation::Maintenance => {
                action_sha256.is_some()
                    && full_member_root_sha256.is_some()
                    && policy_report_sha256.is_some()
                    && authorization_sha256.is_none()
                    && transfer_sha256.is_none()
                    && evaluation_sha256.is_none()
                    && qualification_sha256.is_none()
            }
            GenericActivation::Qualified => {
                evaluation_sha256.is_some()
                    && qualification_sha256.is_some()
                    && authorization_sha256.is_none()
                    && transfer_sha256.is_none()
                    && action_sha256.is_none()
                    && full_member_root_sha256.is_none()
                    && policy_report_sha256.is_none()
            }
        };
        // reject incomplete activation-mode identity sets
        if !mode_identity_valid {
            return None;
        }
        return Some(GenericRuntime {
            value: normalized,
            state: RuntimeState::Active,
            activation: Some(activation),
            authorization_sha256,
            candidate_sha256: Some(candidate_sha256),
            enabled_metrics,
            evaluation_sha256,
            qualification_sha256,
            transfer_sha256,
            action_sha256,
            full_member_root_sha256,
            policy_report_sha256,
        });
    }
    let reason = text_field(&normalized, "reasonCode")?;
    // accept one complete disabled identity
    if state_text != "disabled"
        || !is_generic_reason(&reason)
        || !enabled_metrics.is_empty()
        || !null_fields(
            &normalized,
            &[
                "actionSha256",
                "activationMode",
                "activeBundle",
                "authorizationSha256",
                "candidateArtifactSha256",
                "evaluationReportSha256",
                "expiresAt",
                "fullMemberRootSha256",
                "qualificationReceiptSha256",
                "policyReportSha256",
                "transferReportSha256",
            ],
        )
    {
        return None;
    }
    Some(GenericRuntime {
        value: normalized,
        state: RuntimeState::Disabled,
        activation: None,
        authorization_sha256: None,
        candidate_sha256: None,
        enabled_metrics,
        evaluation_sha256: None,
        qualification_sha256: None,
        transfer_sha256: None,
        action_sha256: None,
        full_member_root_sha256: None,
        policy_report_sha256: None,
    })
}

/// Parse one generic row decision against its runtime and raw-record identity.
fn parse_generic_decision(
    value: &JsonValue,
    record: &RawRecord,
    runtime: &GenericRuntime,
) -> Option<JsonValue> {
    // active rows require the mode-specific closed envelope
    if text_field(value, "state").as_deref() == Some("active") {
        return parse_generic_active_decision(value, record, runtime);
    }
    parse_generic_fail_raw_decision(value)
}

/// Parse one exact unchanged-raw generic decision.
fn parse_generic_fail_raw_decision(value: &JsonValue) -> Option<JsonValue> {
    let adjusted = object_field(value, "adjustedMetrics")?;
    let applied = array_field(value, "appliedMetrics")?;
    let reason = text_field(value, "reasonCode")?;
    // require the exact empty fail-raw contract
    if !exact_keys(value, GENERIC_FAIL_RAW_KEYS)
        || text_field(value, "contractVersion")? != "forecast-adjustment-decision/v1"
        || !matches!(
            text_field(value, "state")?.as_str(),
            "disabled" | "not_applicable"
        )
        || !is_generic_reason(&reason)
        || !object_is_empty(adjusted)
        || !applied.is_empty()
    {
        return None;
    }
    Some(value.clone())
}

/// Parse one mode-specific active generic decision.
fn parse_generic_active_decision(
    value: &JsonValue,
    record: &RawRecord,
    runtime: &GenericRuntime,
) -> Option<JsonValue> {
    let activation = runtime.activation?;
    let expected_keys = match activation {
        GenericActivation::Qualified => GENERIC_ACTIVE_KEYS,
        GenericActivation::WindCanary => GENERIC_WIND_ACTIVE_KEYS,
        GenericActivation::Maintenance => GENERIC_MAINTENANCE_ACTIVE_KEYS,
    };
    // require the exact active envelope and runtime cross-links
    if !exact_keys(value, expected_keys)
        || text_field(value, "contractVersion")? != "forecast-adjustment-decision/v1"
        || text_field(value, "algorithmContractVersion")? != "robust-hierarchical-median/v1"
        || text_field(value, "state")? != "active"
        || !matches!(field(value, "reasonCode"), Some(JsonValue::Null))
        || runtime.state != RuntimeState::Active
        || nullable_text_field(value, "candidateArtifactSha256")? != runtime.candidate_sha256
        || !generic_mode_cross_links(value, runtime, activation)
    {
        return None;
    }
    let adjusted = object_field(value, "adjustedMetrics")?;
    let provenance = object_field(value, "rawForecastProvenance")?;
    // bind the closed provenance envelope to the displayed raw row
    if !exact_keys(provenance, GENERIC_PROVENANCE_KEYS)
        || text_field(provenance, "cohort")? != "legacy_v4_retrieval_snapshot"
        || text_field(provenance, "referenceKind")? != "retrieval_snapshot"
        || !bounded_text_field(provenance, "adapterVersion")
        || !bounded_text_field(provenance, "contractEpoch")
        || !bounded_text_field(provenance, "dataset")
        || !bounded_text_field(provenance, "sourceKey")
        || !bounded_text_field(provenance, "upstreamModel")
        || hash_field(provenance, "sourceConfigFingerprint").is_none()
    {
        return None;
    }
    let target_lead = number_field(provenance, "targetLeadHours")?;
    let target_lead = safe_i64(target_lead)?;
    let lead_band = lead_band(target_lead)?;
    let reference_at_text = text_field(provenance, "referenceAt")?;
    let valid_at_text = text_field(provenance, "validAt")?;
    let reference_at = parse_instant(&reference_at_text)?;
    let valid_at = parse_instant(&valid_at_text)?;
    let lead_ms = valid_at - reference_at;
    let derived_lead = lead_ms.div_euclid(HOUR_MS) + i64::from(lead_ms.rem_euclid(HOUR_MS) != 0);
    // preserve the browser's exact row and derived-lead identity checks
    if derived_lead != target_lead
        || text_field(value, "leadBand")? != lead_band
        || valid_at_text != record.valid_at
        || text_field(provenance, "sourceKey")? != record.source_key
        || Some(text_field(provenance, "dataset")?) != record.provider_dataset
        || Some(text_field(provenance, "upstreamModel")?) != record.upstream_model
    {
        return None;
    }
    let applied = array_field(value, "appliedMetrics")?;
    let mut unique = Vec::with_capacity(applied.len());
    // validate every adjusted metric contribution and reject duplicates
    for item in applied {
        let metric = GenericMetric::parse(&string_value(item)?)?;
        if !runtime.enabled_metrics.contains(&metric)
            || (activation != GenericActivation::Qualified && !metric.is_wind())
            || unique.contains(&metric)
        {
            return None;
        }
        let raw = record.metrics.generic(metric)?;
        let adjusted_value = number_field(adjusted, metric.name())?;
        // require usable raw and physically bounded adjusted values
        if !raw.is_finite() || !metric.admits(adjusted_value) {
            return None;
        }
        unique.push(metric);
    }
    let JsonValue::Object(adjusted_fields) = adjusted else {
        return None;
    };
    // require one nonempty exact adjusted metric set
    if unique.is_empty()
        || adjusted_fields.len() != unique.len()
        || adjusted_fields.iter().any(|(name, _)| {
            String::from_utf16(name)
                .ok()
                .and_then(|name| GenericMetric::parse(&name))
                .is_none_or(|metric| !unique.contains(&metric))
        })
    {
        return None;
    }
    Some(value.clone())
}

/// Validate mode-specific active decision hashes against the admitted runtime.
fn generic_mode_cross_links(
    value: &JsonValue,
    runtime: &GenericRuntime,
    activation: GenericActivation,
) -> bool {
    match activation {
        GenericActivation::WindCanary => {
            text_field(value, "activationKind").as_deref() == Some("wind_transfer_canary")
                && nullable_text_field(value, "authorizationSha256")
                    == Some(runtime.authorization_sha256.clone())
                && nullable_text_field(value, "transferReportSha256")
                    == Some(runtime.transfer_sha256.clone())
        }
        GenericActivation::Maintenance => {
            text_field(value, "activationKind").as_deref() == Some("maintenance_qualified")
                && nullable_text_field(value, "actionSha256") == Some(runtime.action_sha256.clone())
                && nullable_text_field(value, "fullMemberRootSha256")
                    == Some(runtime.full_member_root_sha256.clone())
                && nullable_text_field(value, "policyReportSha256")
                    == Some(runtime.policy_report_sha256.clone())
        }
        GenericActivation::Qualified => {
            nullable_text_field(value, "evaluationReportSha256")
                == Some(runtime.evaluation_sha256.clone())
                && nullable_text_field(value, "qualificationReceiptSha256")
                    == Some(runtime.qualification_sha256.clone())
        }
    }
}

/// Parse the independent temperature runtime including the legacy augmentation.
fn parse_temperature_runtime(value: &JsonValue) -> Option<TemperatureRuntime> {
    let mut normalized = value.clone();
    // augment only the exact predecessor temperature runtime shape
    if exact_keys(value, TEMPERATURE_RUNTIME_LEGACY_KEYS) {
        set_object_field(&mut normalized, "actionSha256", JsonValue::Null);
        let activation = if text_field(value, "state").as_deref() == Some("active") {
            json_string("temperature_canary")
        } else {
            JsonValue::Null
        };
        set_object_field(&mut normalized, "activationMode", activation);
        set_object_field(&mut normalized, "fullMemberRootSha256", JsonValue::Null);
        set_object_field(&mut normalized, "policyReportSha256", JsonValue::Null);
    }
    // require one exact runtime envelope and loaded clock
    if !exact_keys(&normalized, TEMPERATURE_RUNTIME_KEYS)
        || parse_instant(&text_field(&normalized, "loadedAt")?).is_none()
        || !validate_temperature_runtime_source(field(&normalized, "source")?)
    {
        return None;
    }
    let state = text_field(&normalized, "state")?;
    // accept one complete active selection
    if state == "active" {
        let activation = match text_field(&normalized, "activationMode")?.as_str() {
            "temperature_canary" => TemperatureActivation::Canary,
            "maintenance_qualified" => TemperatureActivation::Maintenance,
            _ => return None,
        };
        let active_bundle = hash_field(&normalized, "activeBundle")?;
        let expires_at = nullable_text_field(&normalized, "expiresAt")?;
        // require a valid optional expiry and redacted active reason
        if expires_at
            .as_deref()
            .is_some_and(|instant| parse_instant(instant).is_none())
            || !matches!(field(&normalized, "reasonCode"), Some(JsonValue::Null))
        {
            return None;
        }
        let authorization = optional_hash_field(&normalized, "authorizationSha256")?;
        let action = optional_hash_field(&normalized, "actionSha256")?;
        let root = optional_hash_field(&normalized, "fullMemberRootSha256")?;
        let policy = optional_hash_field(&normalized, "policyReportSha256")?;
        let valid_identity = match activation {
            TemperatureActivation::Canary => {
                authorization.is_some() && action.is_none() && root.is_none() && policy.is_none()
            }
            TemperatureActivation::Maintenance => {
                authorization.is_none() && action.is_some() && root.is_some() && policy.is_some()
            }
        };
        // reject incomplete activation-mode identity sets
        if !valid_identity {
            return None;
        }
        return Some(TemperatureRuntime {
            value: normalized,
            state: RuntimeState::Active,
            active_bundle: Some(active_bundle),
        });
    }
    let reason = text_field(&normalized, "reasonCode")?;
    // accept one fully redacted disabled selection
    if state != "disabled"
        || !is_temperature_reason(&reason)
        || !null_fields(
            &normalized,
            &[
                "actionSha256",
                "activationMode",
                "activeBundle",
                "authorizationSha256",
                "expiresAt",
                "fullMemberRootSha256",
                "policyReportSha256",
                "source",
            ],
        )
    {
        return None;
    }
    Some(TemperatureRuntime {
        value: normalized,
        state: RuntimeState::Disabled,
        active_bundle: None,
    })
}

/// Validate one nullable temperature collector summary.
fn validate_temperature_runtime_source(value: &JsonValue) -> bool {
    // preserve a genuine null collector summary
    if matches!(value, JsonValue::Null) {
        return true;
    }
    let Some(source) = as_object(value) else {
        return false;
    };
    let Some(hour_count) = number_field(source, "hourCount") else {
        return false;
    };
    let Some(status) = text_field(source, "stateStatus") else {
        return false;
    };
    let adaptive = matches!(
        field(source, "adaptiveReady"),
        Some(JsonValue::Boolean(true))
    );
    let adaptive_is_boolean = matches!(field(source, "adaptiveReady"), Some(JsonValue::Boolean(_)));
    let Some(reason) = nullable_text_field(source, "stateReason") else {
        return false;
    };
    let reason_valid = reason.is_none_or(|reason| is_bounded_text(&reason));
    exact_keys(source, TEMPERATURE_RUNTIME_SOURCE_KEYS)
        && adaptive_is_boolean
        && parse_instant(&text_field(source, "firstReceivedAt").unwrap_or_default()).is_some()
        && is_safe_integer(hour_count)
        && (0.0..=19.0).contains(&hour_count)
        && parse_instant(&text_field(source, "latestRunInitializedAt").unwrap_or_default())
            .is_some()
        && reason_valid
        && matches!(
            status.as_str(),
            "cold" | "insufficient" | "invalid" | "supported"
        )
        && adaptive == (status == "supported")
}

/// Parse one independent temperature decision against its runtime and raw row.
fn parse_temperature_decision(
    value: &JsonValue,
    record: &RawRecord,
    runtime: &TemperatureRuntime,
) -> Option<JsonValue> {
    // require one exact decision envelope and raw Best Match binding
    if !exact_keys(value, TEMPERATURE_DECISION_KEYS)
        || text_field(value, "contractVersion")? != "forecast-temperature-canary-decision/v1"
        || nullable_number_field(value, "rawBestMatchTemperatureC")? != record.metrics.temperature_c
    {
        return None;
    }
    let state = text_field(value, "state")?;
    // accept only fully linked active decisions
    if state == "active" {
        validate_temperature_source(field(value, "sourceForecast")?, record)?;
        let corrected = number_field(value, "correctedTemperatureC")?;
        if runtime.state != RuntimeState::Active
            || nullable_text_field(value, "bundleSha256")? != runtime.active_bundle
            || !matches!(text_field(value, "branch")?.as_str(), "adaptive" | "direct")
            || !(-100.0..=70.0).contains(&corrected)
            || !corrected.is_finite()
            || !matches!(field(value, "reasonCode"), Some(JsonValue::Null))
            || hash_field(value, "recentErrorStateSha256").is_none()
        {
            return None;
        }
        return Some(value.clone());
    }
    let reason = text_field(value, "reasonCode")?;
    // accept schema-complete disabled and raw-fallback decisions only
    if !is_temperature_reason(&reason)
        || !matches!(field(value, "branch"), Some(JsonValue::Null))
        || !matches!(field(value, "correctedTemperatureC"), Some(JsonValue::Null))
        || !matches!(state.as_str(), "disabled" | "raw_fallback")
    {
        return None;
    }
    // disabled rows must be fully redacted under a disabled runtime
    if state == "disabled"
        && (runtime.state != RuntimeState::Disabled
            || !null_fields(
                value,
                &["bundleSha256", "recentErrorStateSha256", "sourceForecast"],
            ))
    {
        return None;
    }
    // raw fallbacks must remain linked to their active runtime and any retained source
    if state == "raw_fallback" {
        if runtime.state != RuntimeState::Active
            || nullable_text_field(value, "bundleSha256")? != runtime.active_bundle
            || optional_hash_field(value, "recentErrorStateSha256").is_none()
        {
            return None;
        }
        // validate a retained source receipt without requiring one
        if !matches!(field(value, "sourceForecast"), Some(JsonValue::Null)) {
            validate_temperature_source(field(value, "sourceForecast")?, record)?;
        }
    }
    Some(value.clone())
}

/// Validate one exact live ECMWF temperature source receipt.
fn validate_temperature_source(value: &JsonValue, record: &RawRecord) -> Option<()> {
    let source = as_object(value)?;
    // require the complete closed source schema and static identity
    if !exact_keys(source, TEMPERATURE_SOURCE_KEYS)
        || text_field(source, "adapterVersion")? != "open-meteo-ecmwf-single-run/v1"
        || text_field(source, "dataset")? != "single_run"
        || !matches!(text_field(source, "modelCycle")?.as_str(), "49r1" | "50r1")
        || text_field(source, "providerKey")? != "open-meteo"
        || hash_field(source, "providerResponseSha256").is_none()
        || text_field(source, "upstreamModel")? != "ecmwf_ifs"
        || text_field(source, "validAt")? != record.valid_at
    {
        return None;
    }
    let lead = safe_i64(number_field(source, "modelLeadHours")?)?;
    let horizon = safe_i64(number_field(source, "operationalHorizonHours")?)?;
    let raw_temperature = number_field(source, "rawTemperatureC")?;
    let raw_humidity = nullable_number_field(source, "rawRelativeHumidityPercent")?;
    let raw_wind = nullable_number_field(source, "rawWindSpeedMps")?;
    // enforce every physical and operational source bound
    if !(7..=18).contains(&lead)
        || horizon != lead - 6
        || !(-100.0..=70.0).contains(&raw_temperature)
        || raw_humidity.is_some_and(|value| !(0.0..=100.0).contains(&value))
        || raw_wind.is_some_and(|value| !(0.0..=150.0).contains(&value))
    {
        return None;
    }
    let initialized = parse_instant(&text_field(source, "runInitializedAt")?)?;
    let received = parse_instant(&text_field(source, "firstReceivedAt")?)?;
    let valid = parse_instant(&text_field(source, "validAt")?)?;
    // bind receipt ordering and the exact model lead
    if received < initialized || valid - initialized != lead * HOUR_MS {
        return None;
    }
    Some(())
}

/// Parse the independent rain runtime.
fn parse_rain_runtime(value: &JsonValue) -> Option<RainRuntime> {
    // require the exact runtime identity and nullable loaded clock
    if !exact_keys(value, RAIN_RUNTIME_KEYS)
        || nullable_text_field(value, "loadedAt")?
            .as_deref()
            .is_some_and(|instant| parse_instant(instant).is_none())
        || !validate_rain_runtime_source(field(value, "source")?)
    {
        return None;
    }
    let state = text_field(value, "state")?;
    // accept one bound active bundle
    if state == "active" {
        let active_bundle = hash_field(value, "activeBundle")?;
        if !matches!(field(value, "reasonCode"), Some(JsonValue::Null))
            || parse_instant(&text_field(value, "loadedAt")?).is_none()
        {
            return None;
        }
        return Some(RainRuntime {
            value: value.clone(),
            state: RuntimeState::Active,
            active_bundle: Some(active_bundle),
        });
    }
    // accept one explicit disabled boundary
    if state != "disabled"
        || !matches!(field(value, "activeBundle"), Some(JsonValue::Null))
        || !bounded_text_field(value, "reasonCode")
    {
        return None;
    }
    Some(RainRuntime {
        value: value.clone(),
        state: RuntimeState::Disabled,
        active_bundle: None,
    })
}

/// Validate one nullable rain runtime source summary.
fn validate_rain_runtime_source(value: &JsonValue) -> bool {
    // preserve a genuine null source summary
    if matches!(value, JsonValue::Null) {
        return true;
    }
    let Some(source) = as_object(value) else {
        return false;
    };
    let Some(hours) = number_field(source, "hourCount") else {
        return false;
    };
    exact_keys(source, RAIN_RUNTIME_SOURCE_KEYS)
        && parse_instant(&text_field(source, "runInitializedAt").unwrap_or_default()).is_some()
        && parse_instant(&text_field(source, "firstReceivedAt").unwrap_or_default()).is_some()
        && parse_instant(&text_field(source, "decisionAt").unwrap_or_default()).is_some()
        && is_safe_integer(hours)
        && (0.0..=23.0).contains(&hours)
}

/// Parse one rain decision against its runtime and raw row.
fn parse_rain_decision(
    value: &JsonValue,
    record: &RawRecord,
    runtime: &RainRuntime,
) -> Option<JsonValue> {
    // require the exact decision and raw Best Match amount
    if !exact_keys(value, RAIN_DECISION_KEYS)
        || text_field(value, "contractVersion")? != "forecast-rain-adjustment-decision/v1"
        || nullable_number_field(value, "rawBestMatchPrecipitationMm")?
            != record.metrics.precipitation_mm
    {
        return None;
    }
    let state = text_field(value, "state")?;
    // accept only a finite bound active correction
    if state == "active" {
        validate_rain_source(field(value, "sourceForecast")?, record)?;
        let corrected = number_field(value, "correctedPrecipitationMm")?;
        if runtime.state != RuntimeState::Active
            || nullable_text_field(value, "bundleSha256")? != runtime.active_bundle
            || !matches!(field(value, "reasonCode"), Some(JsonValue::Null))
            || !corrected.is_finite()
            || !(0.0..=30.0).contains(&corrected)
        {
            return None;
        }
        return Some(value.clone());
    }
    // require an explicit schema-complete raw fallback state
    if !matches!(state.as_str(), "disabled" | "raw_fallback")
        || !bounded_text_field(value, "reasonCode")
        || !matches!(
            field(value, "correctedPrecipitationMm"),
            Some(JsonValue::Null)
        )
    {
        return None;
    }
    // disabled rows must be fully redacted under a disabled runtime
    if state == "disabled"
        && (runtime.state != RuntimeState::Disabled
            || !null_fields(value, &["bundleSha256", "sourceForecast"]))
    {
        return None;
    }
    // raw fallbacks must remain linked to their active runtime and any retained source
    if state == "raw_fallback" {
        if runtime.state != RuntimeState::Active
            || nullable_text_field(value, "bundleSha256")? != runtime.active_bundle
        {
            return None;
        }
        // validate a retained source receipt without requiring one
        if !matches!(field(value, "sourceForecast"), Some(JsonValue::Null)) {
            validate_rain_source(field(value, "sourceForecast")?, record)?;
        }
    }
    Some(value.clone())
}

/// Validate one causal ECMWF rain source receipt.
fn validate_rain_source(value: &JsonValue, record: &RawRecord) -> Option<()> {
    let source = as_object(value)?;
    // bind the complete source schema and static identity
    if !exact_keys(source, RAIN_SOURCE_KEYS)
        || text_field(source, "validAt")? != record.valid_at
        || text_field(source, "upstreamModel")? != "ecmwf_ifs"
        || text_field(source, "providerKey")? != "open-meteo"
    {
        return None;
    }
    let lead = safe_i64(number_field(source, "modelLeadHours")?)?;
    let precipitation = number_field(source, "rawPrecipitationMm")?;
    // enforce the bounded model horizon and raw amount
    if !(9..=31).contains(&lead)
        || !precipitation.is_finite()
        || !(0.0..=2000.0).contains(&precipitation)
    {
        return None;
    }
    let initialized = parse_instant(&text_field(source, "runInitializedAt")?)?;
    let received = parse_instant(&text_field(source, "firstReceivedAt")?)?;
    let valid = parse_instant(&text_field(source, "validAt")?)?;
    let decision = parse_instant(&text_field(source, "decisionAt")?)?;
    // bind the exact release delay, receipt ordering, and model lead
    if valid - initialized != lead * HOUR_MS
        || received > decision
        || decision - initialized != 8 * HOUR_MS
        || decision > valid
    {
        return None;
    }
    Some(())
}

/// Parse optional response-level adjustment settings.
fn parse_settings(value: &JsonValue) -> Option<AdjustmentSettings> {
    // require the exact versioned three-switch envelope
    if !exact_keys(value, SETTINGS_KEYS) || number_field(value, "version")? != 1.0 {
        return None;
    }
    Some(AdjustmentSettings {
        temperature: boolean_field(value, "temperature")?,
        wind: boolean_field(value, "wind")?,
        rain: boolean_field(value, "rain")?,
    })
}

/// Apply validated settings to one already admitted safe row.
fn project_settings(mut row: JsonValue, settings: Option<AdjustmentSettings>) -> JsonValue {
    let Some(settings) = settings else {
        return row;
    };
    let projected_generic =
        field(&row, "adjustment").and_then(|decision| project_generic_decision(decision, settings));
    let rain = field(&row, "rainAdjustment").cloned();
    let temperature = field(&row, "temperatureAdjustment").cloned();
    remove_object_field(&mut row, "adjustment");
    remove_object_field(&mut row, "rainAdjustment");
    remove_object_field(&mut row, "temperatureAdjustment");
    // retain only a selected generic decision
    if let Some(decision) = projected_generic {
        set_object_field(&mut row, "adjustment", decision);
    }
    // restore independent groups only while their owning setting remains enabled
    if settings.rain {
        // preserve the complete validated rain decision without synthesizing a fallback
        if let Some(decision) = rain {
            set_object_field(&mut row, "rainAdjustment", decision);
        }
    }
    // retain temperature only under its independent public switch
    if settings.temperature {
        // preserve the complete validated temperature decision including admin-disabled reasons
        if let Some(decision) = temperature {
            set_object_field(&mut row, "temperatureAdjustment", decision);
        }
    }
    row
}

/// Project one validated generic decision through the three public settings switches.
fn project_generic_decision(value: &JsonValue, settings: AdjustmentSettings) -> Option<JsonValue> {
    // retain disabled metadata only while temperature or wind remains visible
    if text_field(value, "state").as_deref() != Some("active") {
        return (settings.temperature || settings.wind).then(|| value.clone());
    }
    let applied = array_field(value, "appliedMetrics")?;
    let adjusted = object_field(value, "adjustedMetrics")?;
    let mut selected_metrics = Vec::new();
    let mut selected_adjusted = Vec::new();
    // copy only metrics enabled for this public view
    for item in applied {
        let metric = GenericMetric::parse(&string_value(item)?)?;
        if settings.admits_metric(metric) {
            selected_metrics.push(item.clone());
            selected_adjusted.push((
                utf16(metric.name()),
                field(adjusted, metric.name())?.clone(),
            ));
        }
    }
    // remove a fully disabled active decision
    if selected_metrics.is_empty() {
        return None;
    }
    let mut projected = value.clone();
    set_object_field(
        &mut projected,
        "appliedMetrics",
        JsonValue::Array(selected_metrics),
    );
    set_object_field(
        &mut projected,
        "adjustedMetrics",
        JsonValue::Object(selected_adjusted),
    );
    Some(projected)
}

/// Construct the browser's generic invalid-metadata runtime fallback.
fn invalid_generic_runtime() -> JsonValue {
    object(vec![
        ("activationMode", JsonValue::Null),
        ("activeBundle", JsonValue::Null),
        ("authorizationSha256", JsonValue::Null),
        ("candidateArtifactSha256", JsonValue::Null),
        ("enabledMetrics", JsonValue::Array(Vec::new())),
        ("evaluationReportSha256", JsonValue::Null),
        ("expiresAt", JsonValue::Null),
        ("loadedAt", JsonValue::Null),
        ("qualificationReceiptSha256", JsonValue::Null),
        ("reasonCode", json_string("adjustment_error")),
        ("state", json_string("disabled")),
        ("transferReportSha256", JsonValue::Null),
    ])
}

/// Construct the browser's isolated invalid temperature runtime fallback.
fn invalid_temperature_runtime() -> JsonValue {
    object(vec![
        ("actionSha256", JsonValue::Null),
        ("activationMode", JsonValue::Null),
        ("activeBundle", JsonValue::Null),
        ("authorizationSha256", JsonValue::Null),
        ("expiresAt", JsonValue::Null),
        ("fullMemberRootSha256", JsonValue::Null),
        ("loadedAt", JsonValue::Null),
        ("policyReportSha256", JsonValue::Null),
        ("reasonCode", json_string("adjustment_error")),
        ("source", JsonValue::Null),
        ("state", json_string("disabled")),
    ])
}

/// Construct the browser's isolated invalid rain runtime fallback.
fn invalid_rain_runtime() -> JsonValue {
    object(vec![
        ("state", json_string("disabled")),
        ("activeBundle", JsonValue::Null),
        ("reasonCode", json_string("adjustment_error")),
        ("loadedAt", JsonValue::Null),
        ("source", JsonValue::Null),
    ])
}

/// Encode validated settings in the browser's stable field order.
fn settings_value(settings: AdjustmentSettings) -> JsonValue {
    object(vec![
        ("version", JsonValue::Number(1.0)),
        ("temperature", JsonValue::Boolean(settings.temperature)),
        ("wind", JsonValue::Boolean(settings.wind)),
        ("rain", JsonValue::Boolean(settings.rain)),
    ])
}

/// Recognize the browser's closed generic reason-code set.
fn is_generic_reason(value: &str) -> bool {
    matches!(
        value,
        "adjustment_error"
            | "admin_disabled"
            | "bundle_invalid"
            | "bundle_missing"
            | "canary_expired"
            | "canary_killed"
            | "coefficient_missing"
            | "cross_link_mismatch"
            | "direction_calm"
            | "evidence_redundancy_missing"
            | "hash_mismatch"
            | "identity_mismatch"
            | "insufficient_data"
            | "metric_not_enabled"
            | "metric_out_of_bounds"
            | "policy_raw"
            | "qualification_failed"
            | "registry_inactive"
            | "registry_invalid"
            | "runtime_fingerprint_mismatch"
            | "training_envelope_mismatch"
            | "unsupported_lead"
            | "wrong_cohort"
    )
}

/// Recognize the browser's closed temperature reason-code set.
fn is_temperature_reason(value: &str) -> bool {
    matches!(
        value,
        "adjustment_error"
            | "admin_disabled"
            | "bundle_invalid"
            | "bundle_missing"
            | "canary_expired"
            | "canary_killed"
            | "inference_error"
            | "invalid_forecast"
            | "invalid_model"
            | "invalid_recent_error_state"
            | "missing_source_forecast"
            | "model_identity_mismatch"
            | "model_not_supported"
            | "model_not_yet_available"
            | "outside_assumed_delay6_next12"
            | "outside_initialization_first12"
            | "outside_operational_window"
            | "policy_raw"
            | "recent_error_state_as_of_mismatch"
            | "recent_error_state_contains_future_data"
            | "recent_error_state_outside_window"
            | "recent_error_state_run_mismatch"
            | "recent_error_state_source_run_mismatch"
            | "registry_inactive"
            | "registry_invalid"
            | "source_identity_mismatch"
            | "source_not_available"
            | "source_stale"
            | "source_time_mismatch"
            | "strength_band_not_supported"
            | "unsupported_cohort"
    )
}

/// Resolve one exact supported generic lead band.
fn lead_band(hours: i64) -> Option<String> {
    // reject unsupported lead hours
    if !(1..=168).contains(&hours) {
        return None;
    }
    let minimum = ((hours - 1) / 24) * 24 + 1;
    Some(format!("{minimum:03}-{:03}", minimum + 23))
}

/// Parse one explicit-offset instant through the approved native date primitive.
fn parse_instant(value: &str) -> Option<i64> {
    NativeWirePrimitives.parse_explicit_utc_milliseconds(value)
}

/// Require one exact object key set.
fn exact_keys(value: &JsonValue, keys: &[&str]) -> bool {
    matches!(value, JsonValue::Object(fields) if fields.len() == keys.len())
        && keys.iter().all(|key| field(value, key).is_some())
}

/// Require every named property to be JSON null.
fn null_fields(value: &JsonValue, keys: &[&str]) -> bool {
    keys.iter()
        .all(|key| matches!(field(value, key), Some(JsonValue::Null)))
}

/// Require a bounded nonempty contract string property.
fn bounded_text_field(value: &JsonValue, key: &str) -> bool {
    text_field(value, key).is_some_and(|value| is_bounded_text(&value))
}

/// Check one bounded nonempty contract string.
fn is_bounded_text(value: &str) -> bool {
    !value.is_empty() && value.encode_utf16().count() <= 256
}

/// Require one lowercase SHA-256 property.
fn hash_field(value: &JsonValue, key: &str) -> Option<String> {
    let value = text_field(value, key)?;
    is_sha256(&value).then_some(value)
}

/// Decode a nullable SHA-256 property while distinguishing malformed input.
fn optional_hash_field(value: &JsonValue, key: &str) -> Option<Option<String>> {
    let value = nullable_text_field(value, key)?;
    // reject a non-null malformed hash
    if value.as_deref().is_some_and(|value| !is_sha256(value)) {
        return None;
    }
    Some(value)
}

/// Recognize one exact lowercase SHA-256 spelling.
fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Convert one JavaScript-safe integer to its exact native representation.
fn safe_i64(value: f64) -> Option<i64> {
    is_safe_integer(value).then_some(value as i64)
}

/// Recognize one JavaScript-safe integer represented by JSON number storage.
fn is_safe_integer(value: f64) -> bool {
    value.is_finite() && value.fract() == 0.0 && value.abs() <= MAX_SAFE_INTEGER
}

/// Borrow one JSON object.
fn as_object(value: &JsonValue) -> Option<&JsonValue> {
    matches!(value, JsonValue::Object(_)).then_some(value)
}

/// Borrow one named JSON object property.
fn object_field<'a>(value: &'a JsonValue, key: &str) -> Option<&'a JsonValue> {
    as_object(field(value, key)?)
}

/// Borrow one named JSON array property.
fn array_field<'a>(value: &'a JsonValue, key: &str) -> Option<&'a [JsonValue]> {
    let JsonValue::Array(values) = field(value, key)? else {
        return None;
    };
    Some(values)
}

/// Decode one named JSON string property without lossy UTF-16 replacement.
fn text_field(value: &JsonValue, key: &str) -> Option<String> {
    string_value(field(value, key)?)
}

/// Decode one JSON string without lossy UTF-16 replacement.
fn string_value(value: &JsonValue) -> Option<String> {
    let JsonValue::String(value) = value else {
        return None;
    };
    String::from_utf16(value).ok()
}

/// Decode one nullable JSON string property.
fn nullable_text_field(value: &JsonValue, key: &str) -> Option<Option<String>> {
    match field(value, key)? {
        JsonValue::Null => Some(None),
        JsonValue::String(value) => Some(Some(String::from_utf16(value).ok()?)),
        _ => None,
    }
}

/// Decode one finite JSON number property.
fn number_field(value: &JsonValue, key: &str) -> Option<f64> {
    let JsonValue::Number(value) = field(value, key)? else {
        return None;
    };
    value.is_finite().then_some(*value)
}

/// Decode one finite-or-null JSON number property.
fn nullable_number_field(value: &JsonValue, key: &str) -> Option<Option<f64>> {
    match field(value, key)? {
        JsonValue::Null => Some(None),
        JsonValue::Number(value) if value.is_finite() => Some(Some(*value)),
        _ => None,
    }
}

/// Decode one boolean property.
fn boolean_field(value: &JsonValue, key: &str) -> Option<bool> {
    let JsonValue::Boolean(value) = field(value, key)? else {
        return None;
    };
    Some(*value)
}

/// Report whether one JSON object has no fields.
fn object_is_empty(value: &JsonValue) -> bool {
    matches!(value, JsonValue::Object(fields) if fields.is_empty())
}

/// Clone one object while removing private adjustment properties.
fn object_without(value: &JsonValue, removed: &[&str]) -> Option<JsonValue> {
    let JsonValue::Object(fields) = value else {
        return None;
    };
    Some(JsonValue::Object(
        fields
            .iter()
            .filter(|(name, _)| {
                String::from_utf16(name)
                    .ok()
                    .is_none_or(|name| !removed.contains(&name.as_str()))
            })
            .cloned()
            .collect(),
    ))
}

/// Insert or replace one object property without disturbing unrelated source fields.
fn set_object_field(value: &mut JsonValue, key: &str, replacement: JsonValue) {
    let JsonValue::Object(fields) = value else {
        return;
    };
    // replace the source property in place when it already exists
    if let Some((_, current)) = fields
        .iter_mut()
        .find(|(name, _)| name.iter().copied().eq(key.encode_utf16()))
    {
        *current = replacement;
        return;
    }
    fields.push((utf16(key), replacement));
}

/// Remove one object property while preserving every unrelated field and its order.
fn remove_object_field(value: &mut JsonValue, key: &str) {
    let JsonValue::Object(fields) = value else {
        return;
    };
    fields.retain(|(name, _)| !name.iter().copied().eq(key.encode_utf16()));
}

/// Construct one ordered JSON object from static field names.
fn object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(name, value)| (utf16(name), value))
            .collect(),
    )
}

/// Construct one JSON string from valid Rust UTF-8.
fn json_string(value: &str) -> JsonValue {
    JsonValue::String(utf16(value))
}

/// Encode one Rust string as the generated contract's UTF-16 storage.
fn utf16(value: &str) -> Vec<u16> {
    value.encode_utf16().collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use std::io::Write as _;
    use std::path::PathBuf;
    use std::process::{Command, Stdio};

    use super::*;

    const NODE_PARSE: &str = r#"
import { readFileSync } from "node:fs";
import { parseForecastRecordsResponse } from "./apps/web/dist/index.js";
const fixture = JSON.parse(readFileSync(0, "utf8"));
process.stdout.write(JSON.stringify(parseForecastRecordsResponse(fixture)));
"#;

    #[derive(Clone, Copy)]
    pub(crate) enum FixtureMode {
        Active,
        Disabled,
        ContextFallback,
    }

    /// Match active, admin-disabled, and context-triggered family fallback against compiled TS.
    #[test]
    fn forecast_admission_matches_compiled_typescript_family_boundaries() {
        // exercise every source-backed decision state through the same production parser
        for mode in [
            FixtureMode::Active,
            FixtureMode::Disabled,
            FixtureMode::ContextFallback,
        ] {
            let input = fixture(mode);
            let expected = run_node_parse(&input);
            let actual = admit_native_widget_forecast(&input)
                .expect("native forecast admission")
                .response;
            assert_eq!(actual, expected);
        }
    }

    /// Preserve genuine admin-disabled decisions while failing all malformed context families raw.
    #[test]
    fn forecast_admission_preserves_disabled_and_global_fallback_states() {
        let disabled = admit_native_widget_forecast(&fixture(FixtureMode::Disabled))
            .expect("disabled admission")
            .response;
        let disabled_row = first_row(&disabled);
        assert_eq!(
            text_field(
                object_field(disabled_row, "adjustment").unwrap(),
                "reasonCode"
            )
            .as_deref(),
            Some("admin_disabled")
        );
        assert_eq!(
            text_field(
                object_field(disabled_row, "temperatureAdjustment").unwrap(),
                "reasonCode"
            )
            .as_deref(),
            Some("admin_disabled")
        );
        assert_eq!(
            text_field(
                object_field(disabled_row, "rainAdjustment").unwrap(),
                "reasonCode"
            )
            .as_deref(),
            Some("admin_disabled")
        );

        let fallback = admit_native_widget_forecast(&fixture(FixtureMode::ContextFallback))
            .expect("fallback admission")
            .response;
        let fallback_row = first_row(&fallback);
        // one malformed AQ-context row invalidates every decision in its complete family
        for name in ["adjustment", "temperatureAdjustment", "rainAdjustment"] {
            assert!(field(fallback_row, name).is_none());
        }
        assert_eq!(
            text_field(
                object_field(&fallback, "adjustmentRuntime").unwrap(),
                "reasonCode"
            )
            .as_deref(),
            Some("adjustment_error")
        );
    }

    /// Build one complete response accepted by the original browser parser.
    pub(crate) fn fixture(mode: FixtureMode) -> JsonValue {
        let context_malformed = matches!(mode, FixtureMode::ContextFallback);
        object(vec![
            ("adjustmentRuntime", generic_runtime(mode)),
            (
                "adjustmentSettings",
                settings_value(AdjustmentSettings {
                    temperature: true,
                    wind: true,
                    rain: true,
                }),
            ),
            (
                "airQualityContext",
                JsonValue::Array(vec![weather_row(mode, context_malformed)]),
            ),
            ("data", JsonValue::Array(vec![weather_row(mode, false)])),
            ("generatedAt", json_string("2026-09-12T20:05:00.000Z")),
            ("pressureContext", JsonValue::Array(Vec::new())),
            ("rainAdjustmentRuntime", rain_runtime(mode)),
            (
                "site",
                object(vec![
                    ("latitude", JsonValue::Number(47.950_429_954_185_445)),
                    ("longitude", JsonValue::Number(-122.427_970_126_081_93)),
                    ("name", json_string("Ballydidean")),
                    ("slug", json_string("ballydidean")),
                    ("timezone", json_string("America/Los_Angeles")),
                ]),
            ),
            ("temperatureAdjustmentRuntime", temperature_runtime(mode)),
        ])
    }

    /// Build one raw forecast row and its complete family decisions.
    fn weather_row(mode: FixtureMode, malformed: bool) -> JsonValue {
        object(vec![
            (
                "adjustment",
                if malformed {
                    JsonValue::Null
                } else {
                    generic_decision(mode)
                },
            ),
            (
                "freshness",
                object(vec![
                    ("ageSeconds", JsonValue::Number(300.0)),
                    ("label", json_string("5 minutes ago")),
                    ("status", json_string("fresh")),
                ]),
            ),
            ("id", json_string("open-meteo-forecast-20260912T21")),
            (
                "metadata",
                object(vec![
                    ("device", JsonValue::Null),
                    (
                        "provider",
                        object(vec![
                            ("dataset", json_string("best_match")),
                            ("elevationM", JsonValue::Null),
                            ("gridCell", JsonValue::Null),
                            ("propertySensors", JsonValue::Null),
                        ]),
                    ),
                    ("quality", JsonValue::Null),
                    (
                        "upstream",
                        object(vec![
                            ("model", json_string("best_match")),
                            ("timezone", json_string("UTC")),
                        ]),
                    ),
                ]),
            ),
            ("metrics", metrics()),
            ("productRunAt", json_string("2026-09-12T12:00:00.000Z")),
            (
                "provenance",
                object(vec![
                    (
                        "attribution",
                        object(vec![
                            ("label", json_string("Open-Meteo")),
                            ("url", json_string("https://open-meteo.com/")),
                        ]),
                    ),
                    ("label", json_string("Open-Meteo forecast")),
                    ("providerKey", json_string("open-meteo")),
                    ("sourceId", json_string("best-match-20260912T12")),
                    ("sourceKey", json_string("open-meteo:forecast")),
                    ("sourceKind", json_string("forecast")),
                    ("stationSlug", json_string("open-meteo-virtual")),
                ]),
            ),
            (
                "rainAdjustment",
                if malformed {
                    JsonValue::Null
                } else {
                    rain_decision(mode)
                },
            ),
            ("receivedAt", json_string("2026-09-12T20:00:00.000Z")),
            ("revisionCount", JsonValue::Number(0.0)),
            (
                "temperatureAdjustment",
                if malformed {
                    JsonValue::Null
                } else {
                    temperature_decision(mode)
                },
            ),
            ("validAt", json_string("2026-09-12T21:00:00.000Z")),
        ])
    }

    /// Build all canonical raw metrics used by the browser record validator.
    fn metrics() -> JsonValue {
        object(vec![
            ("apparentTemperatureC", JsonValue::Number(9.0)),
            ("blackGlobeTemperatureC", JsonValue::Null),
            ("cloudCoverPercent", JsonValue::Number(20.0)),
            ("pm25MicrogramsPerCubicMeter", JsonValue::Null),
            ("precipitationMm", JsonValue::Number(1.0)),
            ("precipitationRateMmPerHour", JsonValue::Null),
            ("pressureHpa", JsonValue::Number(1012.0)),
            ("relativeHumidityPercent", JsonValue::Number(70.0)),
            (
                "soilElectricalConductivityMicrosiemensPerCm",
                JsonValue::Null,
            ),
            ("soilMoisturePercent", JsonValue::Null),
            ("solarRadiationWm2", JsonValue::Number(100.0)),
            ("temperatureC", JsonValue::Number(10.0)),
            ("uvIndex", JsonValue::Number(1.0)),
            ("windDirectionDegrees", JsonValue::Number(180.0)),
            ("windGustMps", JsonValue::Number(7.0)),
            ("windSpeedMps", JsonValue::Number(4.0)),
            ("wetBulbGlobeTemperatureC", JsonValue::Null),
        ])
    }

    /// Build one exact active or disabled generic runtime.
    fn generic_runtime(mode: FixtureMode) -> JsonValue {
        let active = !matches!(mode, FixtureMode::Disabled);
        object(vec![
            ("actionSha256", JsonValue::Null),
            (
                "activationMode",
                if active {
                    json_string("qualified")
                } else {
                    JsonValue::Null
                },
            ),
            (
                "activeBundle",
                if active {
                    json_string(&digest('a'))
                } else {
                    JsonValue::Null
                },
            ),
            ("authorizationSha256", JsonValue::Null),
            (
                "candidateArtifactSha256",
                if active {
                    json_string(&digest('b'))
                } else {
                    JsonValue::Null
                },
            ),
            (
                "enabledMetrics",
                if active {
                    JsonValue::Array(vec![
                        json_string("temperatureC"),
                        json_string("windSpeedMps"),
                    ])
                } else {
                    JsonValue::Array(Vec::new())
                },
            ),
            (
                "evaluationReportSha256",
                if active {
                    json_string(&digest('c'))
                } else {
                    JsonValue::Null
                },
            ),
            ("expiresAt", JsonValue::Null),
            ("fullMemberRootSha256", JsonValue::Null),
            ("loadedAt", json_string("2026-09-12T20:00:00.000Z")),
            (
                "qualificationReceiptSha256",
                if active {
                    json_string(&digest('d'))
                } else {
                    JsonValue::Null
                },
            ),
            (
                "reasonCode",
                if active {
                    JsonValue::Null
                } else {
                    json_string("admin_disabled")
                },
            ),
            (
                "state",
                json_string(if active { "active" } else { "disabled" }),
            ),
            ("policyReportSha256", JsonValue::Null),
            ("transferReportSha256", JsonValue::Null),
        ])
    }

    /// Build one exact active or disabled generic decision.
    fn generic_decision(mode: FixtureMode) -> JsonValue {
        // disabled rows retain the genuine public admin reason
        if matches!(mode, FixtureMode::Disabled) {
            return object(vec![
                ("adjustedMetrics", object(Vec::new())),
                ("appliedMetrics", JsonValue::Array(Vec::new())),
                (
                    "contractVersion",
                    json_string("forecast-adjustment-decision/v1"),
                ),
                ("reasonCode", json_string("admin_disabled")),
                ("state", json_string("disabled")),
            ]);
        }
        object(vec![
            (
                "adjustedMetrics",
                object(vec![
                    ("temperatureC", JsonValue::Number(12.0)),
                    ("windSpeedMps", JsonValue::Number(5.0)),
                ]),
            ),
            (
                "algorithmContractVersion",
                json_string("robust-hierarchical-median/v1"),
            ),
            (
                "appliedMetrics",
                JsonValue::Array(vec![
                    json_string("temperatureC"),
                    json_string("windSpeedMps"),
                ]),
            ),
            ("candidateArtifactSha256", json_string(&digest('b'))),
            (
                "contractVersion",
                json_string("forecast-adjustment-decision/v1"),
            ),
            ("evaluationReportSha256", json_string(&digest('c'))),
            ("leadBand", json_string("001-024")),
            ("qualificationReceiptSha256", json_string(&digest('d'))),
            (
                "rawForecastProvenance",
                object(vec![
                    ("adapterVersion", json_string("open-meteo/v1")),
                    ("cohort", json_string("legacy_v4_retrieval_snapshot")),
                    ("contractEpoch", json_string("2026-09")),
                    ("dataset", json_string("best_match")),
                    ("referenceAt", json_string("2026-09-12T12:00:00.000Z")),
                    ("referenceKind", json_string("retrieval_snapshot")),
                    ("sourceConfigFingerprint", json_string(&digest('1'))),
                    ("sourceKey", json_string("open-meteo:forecast")),
                    ("targetLeadHours", JsonValue::Number(9.0)),
                    ("upstreamModel", json_string("best_match")),
                    ("validAt", json_string("2026-09-12T21:00:00.000Z")),
                ]),
            ),
            ("reasonCode", JsonValue::Null),
            ("state", json_string("active")),
        ])
    }

    /// Build one exact active or disabled temperature runtime.
    fn temperature_runtime(mode: FixtureMode) -> JsonValue {
        let active = !matches!(mode, FixtureMode::Disabled);
        object(vec![
            ("actionSha256", JsonValue::Null),
            (
                "activationMode",
                if active {
                    json_string("temperature_canary")
                } else {
                    JsonValue::Null
                },
            ),
            (
                "activeBundle",
                if active {
                    json_string(&digest('e'))
                } else {
                    JsonValue::Null
                },
            ),
            (
                "authorizationSha256",
                if active {
                    json_string(&digest('f'))
                } else {
                    JsonValue::Null
                },
            ),
            ("expiresAt", JsonValue::Null),
            ("fullMemberRootSha256", JsonValue::Null),
            ("loadedAt", json_string("2026-09-12T20:00:00.000Z")),
            ("policyReportSha256", JsonValue::Null),
            (
                "reasonCode",
                if active {
                    JsonValue::Null
                } else {
                    json_string("admin_disabled")
                },
            ),
            ("source", JsonValue::Null),
            (
                "state",
                json_string(if active { "active" } else { "disabled" }),
            ),
        ])
    }

    /// Build one exact active or disabled temperature decision.
    fn temperature_decision(mode: FixtureMode) -> JsonValue {
        // disabled rows retain the complete redacted decision
        if matches!(mode, FixtureMode::Disabled) {
            return object(vec![
                ("branch", JsonValue::Null),
                ("bundleSha256", JsonValue::Null),
                (
                    "contractVersion",
                    json_string("forecast-temperature-canary-decision/v1"),
                ),
                ("correctedTemperatureC", JsonValue::Null),
                ("rawBestMatchTemperatureC", JsonValue::Number(10.0)),
                ("reasonCode", json_string("admin_disabled")),
                ("recentErrorStateSha256", JsonValue::Null),
                ("sourceForecast", JsonValue::Null),
                ("state", json_string("disabled")),
            ]);
        }
        object(vec![
            ("branch", json_string("direct")),
            ("bundleSha256", json_string(&digest('e'))),
            (
                "contractVersion",
                json_string("forecast-temperature-canary-decision/v1"),
            ),
            ("correctedTemperatureC", JsonValue::Number(11.0)),
            ("rawBestMatchTemperatureC", JsonValue::Number(10.0)),
            ("reasonCode", JsonValue::Null),
            ("recentErrorStateSha256", json_string(&digest('8'))),
            ("sourceForecast", temperature_source()),
            ("state", json_string("active")),
        ])
    }

    /// Build one causal source receipt for an active temperature decision.
    fn temperature_source() -> JsonValue {
        object(vec![
            (
                "adapterVersion",
                json_string("open-meteo-ecmwf-single-run/v1"),
            ),
            ("dataset", json_string("single_run")),
            ("firstReceivedAt", json_string("2026-09-12T12:10:00.000Z")),
            ("modelCycle", json_string("49r1")),
            ("modelLeadHours", JsonValue::Number(9.0)),
            ("operationalHorizonHours", JsonValue::Number(3.0)),
            ("providerKey", json_string("open-meteo")),
            ("providerResponseSha256", json_string(&digest('9'))),
            ("rawRelativeHumidityPercent", JsonValue::Number(70.0)),
            ("rawTemperatureC", JsonValue::Number(10.0)),
            ("rawWindSpeedMps", JsonValue::Number(4.0)),
            ("runInitializedAt", json_string("2026-09-12T12:00:00.000Z")),
            ("upstreamModel", json_string("ecmwf_ifs")),
            ("validAt", json_string("2026-09-12T21:00:00.000Z")),
        ])
    }

    /// Build one exact active or disabled rain runtime.
    fn rain_runtime(mode: FixtureMode) -> JsonValue {
        let active = !matches!(mode, FixtureMode::Disabled);
        object(vec![
            (
                "state",
                json_string(if active { "active" } else { "disabled" }),
            ),
            (
                "activeBundle",
                if active {
                    json_string(&digest('7'))
                } else {
                    JsonValue::Null
                },
            ),
            (
                "reasonCode",
                if active {
                    JsonValue::Null
                } else {
                    json_string("admin_disabled")
                },
            ),
            (
                "loadedAt",
                if active {
                    json_string("2026-09-12T20:00:00.000Z")
                } else {
                    JsonValue::Null
                },
            ),
            ("source", JsonValue::Null),
        ])
    }

    /// Build one exact active or disabled rain decision.
    fn rain_decision(mode: FixtureMode) -> JsonValue {
        // disabled rows retain the complete redacted decision
        if matches!(mode, FixtureMode::Disabled) {
            return object(vec![
                (
                    "contractVersion",
                    json_string("forecast-rain-adjustment-decision/v1"),
                ),
                ("state", json_string("disabled")),
                ("reasonCode", json_string("admin_disabled")),
                ("bundleSha256", JsonValue::Null),
                ("correctedPrecipitationMm", JsonValue::Null),
                ("rawBestMatchPrecipitationMm", JsonValue::Number(1.0)),
                ("sourceForecast", JsonValue::Null),
            ]);
        }
        object(vec![
            (
                "contractVersion",
                json_string("forecast-rain-adjustment-decision/v1"),
            ),
            ("state", json_string("active")),
            ("reasonCode", JsonValue::Null),
            ("bundleSha256", json_string(&digest('7'))),
            ("correctedPrecipitationMm", JsonValue::Number(0.5)),
            ("rawBestMatchPrecipitationMm", JsonValue::Number(1.0)),
            (
                "sourceForecast",
                object(vec![
                    ("runInitializedAt", json_string("2026-09-12T12:00:00.000Z")),
                    ("firstReceivedAt", json_string("2026-09-12T19:00:00.000Z")),
                    ("validAt", json_string("2026-09-12T21:00:00.000Z")),
                    ("rawPrecipitationMm", JsonValue::Number(0.5)),
                    ("modelLeadHours", JsonValue::Number(9.0)),
                    ("decisionAt", json_string("2026-09-12T20:00:00.000Z")),
                    ("upstreamModel", json_string("ecmwf_ifs")),
                    ("providerKey", json_string("open-meteo")),
                ]),
            ),
        ])
    }

    /// Produce one exact lowercase digest fixture.
    fn digest(value: char) -> String {
        std::iter::repeat_n(value, 64).collect()
    }

    /// Run the compiled original parser against one complete source envelope.
    fn run_node_parse(input: &JsonValue) -> JsonValue {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let bytes = NativeWirePrimitives
            .encode_ordered_json(input)
            .expect("source fixture JSON");
        let mut child = Command::new("node")
            .args(["--input-type=module", "--eval", NODE_PARSE])
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("compiled TypeScript parser oracle");
        child
            .stdin
            .take()
            .expect("Node stdin")
            .write_all(&bytes)
            .expect("write forecast fixture");
        let output = child.wait_with_output().expect("Node parser output");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        NativeWirePrimitives
            .decode_json(&output.stdout)
            .expect("Node JSON output")
    }

    /// Borrow the first visible row from one admitted response.
    fn first_row(value: &JsonValue) -> &JsonValue {
        let JsonValue::Array(rows) = field(value, "data").expect("forecast data") else {
            panic!("forecast data is not an array")
        };
        &rows[0]
    }
}
