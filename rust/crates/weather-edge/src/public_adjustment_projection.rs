//! The edge's persisted adjustment switches over ordered TS-canonical JSON values.

use std::error::Error;
use std::fmt::{Display, Formatter};
use std::sync::Mutex;

use weather_contracts::JsonValue;

use crate::{
    DISABLED_FORECAST_ADJUSTMENT_SETTINGS, ForecastAdjustmentSettings,
    ForecastAdjustmentSettingsStore, PublicApiProjection, PublicApiResponseProjector,
};

type Object = Vec<(Vec<u16>, JsonValue)>;

/// Approved decoder/encoder for the original source's ordered compact UTF-8 JSON.
pub trait PublicAdjustmentJsonCodec {
    /// Parse one complete API JSON value without discarding insertion order or unknown members.
    fn decode(&self, bytes: &[u8]) -> Option<JsonValue>;

    /// Encode the compact JSON bytes without a trailing newline.
    fn encode(&self, value: &JsonValue) -> Option<Vec<u8>>;
}

/// Access to the same persisted snapshot used by the ordinary admin settings owner.
pub trait PublicAdjustmentSnapshot {
    /// Take one settings snapshot before starting the upstream request.
    fn read_settings(&self) -> ForecastAdjustmentSettings;
}

impl<Store: ForecastAdjustmentSettingsStore> PublicAdjustmentSnapshot for Mutex<Store> {
    /// Read the existing store and preserve its normal fail-closed snapshot behavior.
    fn read_settings(&self) -> ForecastAdjustmentSettings {
        self.lock().map_or(
            DISABLED_FORECAST_ADJUSTMENT_SETTINGS,
            // retain the store's configured or unavailable snapshot independently of its diagnostic
            |mut store| store.read_status().settings,
        )
    }
}

impl<Store: PublicAdjustmentSnapshot + ?Sized> PublicAdjustmentSnapshot for &Store {
    /// Allow the serving and admin owners to share the same underlying snapshot.
    fn read_settings(&self) -> ForecastAdjustmentSettings {
        (*self).read_settings()
    }
}

/// Functional errors returned as the existing upstream-unavailable outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicAdjustmentProjectionError {
    Decode,
    Encode,
    Forecast,
    ForecastRow,
    ForecastDecision,
    RainStatus,
}

impl Display for PublicAdjustmentProjectionError {
    /// Preserve each ordinary projection error category below the response application.
    fn fmt(&self, output: &mut Formatter<'_>) -> std::fmt::Result {
        output.write_str(match self {
            Self::Decode => "upstream adjustment JSON is invalid",
            Self::Encode => "projected adjustment JSON could not be encoded",
            Self::Forecast => "forecast API response is invalid",
            Self::ForecastRow => "forecast row is invalid",
            Self::ForecastDecision => "forecast adjustment decision is invalid",
            Self::RainStatus => "rain collection status is invalid",
        })
    }
}

impl Error for PublicAdjustmentProjectionError {}

/// Concrete business projection between the approved codec and the bounded API proxy.
pub struct PublicAdjustmentProjector<Settings, Codec> {
    settings: Settings,
    codec: Codec,
}

impl<Settings, Codec> PublicAdjustmentProjector<Settings, Codec> {
    /// Bind the existing settings owner and the same codec used for the edge wire.
    pub const fn new(settings: Settings, codec: Codec) -> Self {
        Self { settings, codec }
    }
}

impl<Settings, Codec> PublicApiResponseProjector for PublicAdjustmentProjector<Settings, Codec>
where
    Settings: PublicAdjustmentSnapshot,
    Codec: PublicAdjustmentJsonCodec,
{
    type Error = PublicAdjustmentProjectionError;
    type Snapshot = ForecastAdjustmentSettings;

    /// Take one snapshot before fetch for both supported projected representations.
    fn prepare(&self, _: PublicApiProjection) -> Result<Self::Snapshot, Self::Error> {
        Ok(self.settings.read_settings())
    }

    /// Change only the original edge-owned fields and emit their exact response frame.
    fn project(
        &self,
        projection: PublicApiProjection,
        settings: &Self::Snapshot,
        source: &[u8],
    ) -> Result<Vec<u8>, Self::Error> {
        // ordinary unprojected bytes never require a codec round-trip
        if projection == PublicApiProjection::None {
            return Ok(source.to_vec());
        }
        let mut value = self
            .codec
            .decode(source)
            .ok_or(PublicAdjustmentProjectionError::Decode)?;
        apply_public_adjustment_projection(projection, *settings, &mut value)?;
        let mut body = self
            .codec
            .encode(&value)
            .ok_or(PublicAdjustmentProjectionError::Encode)?;
        body.push(b'\n');
        Ok(body)
    }
}

/// Apply the same per-request snapshot to a decoded forecast or rain aggregate.
pub fn apply_public_adjustment_projection(
    projection: PublicApiProjection,
    settings: ForecastAdjustmentSettings,
    value: &mut JsonValue,
) -> Result<(), PublicAdjustmentProjectionError> {
    match projection {
        PublicApiProjection::None => Ok(()),
        PublicApiProjection::Forecast => project_forecast(value, settings),
        PublicApiProjection::RainCollection => project_rain_collection(value, settings.rain),
    }
}

/// Preserve the complete forecast envelope while projecting its active runtime members.
fn project_forecast(
    value: &mut JsonValue,
    settings: ForecastAdjustmentSettings,
) -> Result<(), PublicAdjustmentProjectionError> {
    use PublicAdjustmentProjectionError as Error;
    let forecast = as_object_mut(value).ok_or(Error::Forecast)?;
    // admit the original hourly data container before changing any returned settings
    if !matches!(get(forecast, "data"), Some(JsonValue::Array(_))) {
        return Err(Error::Forecast);
    }
    set(
        forecast,
        "adjustmentSettings",
        object([
            ("version", JsonValue::Number(f64::from(settings.version))),
            ("temperature", JsonValue::Boolean(settings.temperature)),
            ("wind", JsonValue::Boolean(settings.wind)),
            ("rain", JsonValue::Boolean(settings.rain)),
        ]),
    );
    project_forecast_runtime(forecast, settings);
    // independently replace disabled runtime identities without changing their load clocks
    if !settings.temperature
        && let Some(runtime) =
            get_mut(forecast, "temperatureAdjustmentRuntime").and_then(active_object)
    {
        *runtime = disabled_runtime(runtime, TEMPERATURE_RUNTIME_FIELDS);
    }
    if !settings.rain
        && let Some(runtime) = get_mut(forecast, "rainAdjustmentRuntime").and_then(active_object)
    {
        *runtime = disabled_runtime(runtime, RAIN_RUNTIME_FIELDS);
    }
    let Some(JsonValue::Array(rows)) = get_mut(forecast, "data") else {
        return Err(Error::Forecast);
    };
    // apply the same snapshot only to the source owner's hourly data rows
    for row in rows {
        match row {
            JsonValue::Object(row) => project_forecast_row(row, settings)?,
            // a json array is an object with no named adjustment members in the predecessor
            JsonValue::Array(_) => {}
            _ => return Err(Error::ForecastRow),
        }
    }
    Ok(())
}

/// Limit the generic runtime's active metric list using the approved independent switches.
fn project_forecast_runtime(forecast: &mut Object, settings: ForecastAdjustmentSettings) {
    // mimic the source's conditional projection only for an active runtime with an array
    if let Some(runtime) = get_mut(forecast, "adjustmentRuntime").and_then(active_object)
        && let Some(JsonValue::Array(metrics)) = get_mut(runtime, "enabledMetrics")
    {
        let policy = MetricPolicy::new(settings);
        metrics.retain(
            // preserve the original list order
            |metric| policy.allows(metric),
        );
        // replace the full metadata object only if no requested metrics remain active
        if metrics.is_empty() {
            *runtime = disabled_runtime(runtime, FORECAST_RUNTIME_FIELDS);
        }
    }
}

/// Apply the switches to one row while retaining raw values and unrelated source metadata.
fn project_forecast_row(
    row: &mut Object,
    settings: ForecastAdjustmentSettings,
) -> Result<(), PublicAdjustmentProjectionError> {
    // project ordinary active corrections without changing any other row decision
    if let Some(decision) = get_mut(row, "adjustment").and_then(active_object) {
        project_generic_decision(decision, MetricPolicy::new(settings))?;
    }
    // keep the original object members and order for each independent disabled decision
    if !settings.temperature
        && let Some(decision) = get_mut(row, "temperatureAdjustment").and_then(active_object)
    {
        disable_decision(decision, TEMPERATURE_DECISION_FIELDS);
    }
    if !settings.rain
        && let Some(decision) = get_mut(row, "rainAdjustment").and_then(active_object)
    {
        disable_decision(decision, RAIN_DECISION_FIELDS);
    }
    Ok(())
}

/// Preserve the active generic decision only when at least one permitted metric remains.
fn project_generic_decision(
    decision: &mut Object,
    policy: MetricPolicy,
) -> Result<(), PublicAdjustmentProjectionError> {
    use PublicAdjustmentProjectionError as Error;
    let Some(JsonValue::Array(applied)) = get(decision, "appliedMetrics") else {
        return Err(Error::ForecastDecision);
    };
    let Some(original @ (JsonValue::Object(_) | JsonValue::Array(_))) =
        get(decision, "adjustedMetrics")
    else {
        return Err(Error::ForecastDecision);
    };
    let mut allowed = Vec::with_capacity(applied.len());
    let mut filtered = Vec::with_capacity(applied.len());
    // reconstruct ordered adjusted members in the retained applied metric order
    for metric in applied {
        let JsonValue::String(name) = metric else {
            continue;
        };
        // unapproved metric categories do not contribute a public adjustment
        if !policy.allows(metric) {
            continue;
        }
        allowed.push(metric.clone());
        // omit unavailable properties just as compact json omits javascript undefined
        if let JsonValue::Object(original) = original
            && let Some((_, value)) = original.iter().find(
                // use the original utf16 property identity without lossy string conversion
                |(key, _)| key == name,
            )
        {
            upsert(&mut filtered, name.clone(), value.clone());
        }
    }
    // preserve the source's compact disabled decision when nothing remains applied
    if allowed.is_empty() {
        *decision = object_fields([
            ("adjustedMetrics", JsonValue::Object(Vec::new())),
            ("appliedMetrics", JsonValue::Array(Vec::new())),
            ("contractVersion", string("forecast-adjustment-decision/v1")),
            ("reasonCode", string("admin_disabled")),
            ("state", string("disabled")),
        ]);
    } else {
        set(decision, "appliedMetrics", JsonValue::Array(allowed));
        set(decision, "adjustedMetrics", JsonValue::Object(filtered));
    }
    Ok(())
}

/// Apply the rain switch only to the aggregate's admitted boolean model flag.
fn project_rain_collection(
    value: &mut JsonValue,
    rain: bool,
) -> Result<(), PublicAdjustmentProjectionError> {
    let status = as_object_mut(value).ok_or(PublicAdjustmentProjectionError::RainStatus)?;
    let Some(JsonValue::Boolean(model_enabled)) =
        get_mut(status, "data").and_then(as_object_mut).and_then(
            // find the original aggregate flag without constructing another response
            |data| get_mut(data, "modelEnabled"),
        )
    else {
        return Err(PublicAdjustmentProjectionError::RainStatus);
    };
    *model_enabled &= rain;
    Ok(())
}

#[derive(Clone, Copy)]
struct MetricPolicy(ForecastAdjustmentSettings);

impl MetricPolicy {
    /// Retain the existing category policy from the current settings snapshot.
    const fn new(settings: ForecastAdjustmentSettings) -> Self {
        Self(settings)
    }

    /// Match only the predecessor's adjustment metric names and independent humidity rule.
    fn allows(self, metric: &JsonValue) -> bool {
        let settings = self.0;
        // no correction remains when all three switches are off
        if !settings.temperature && !settings.wind && !settings.rain {
            return false;
        }
        if is_string(metric, "relativeHumidityPercent") {
            return true;
        }
        if is_string(metric, "temperatureC") || is_string(metric, "apparentTemperatureC") {
            return settings.temperature;
        }
        settings.wind
            && ["windDirectionDegrees", "windGustMps", "windSpeedMps"]
                .into_iter()
                .any(
                    // retain only the reviewed wind metric names
                    |name| is_string(metric, name),
                )
    }
}

const FORECAST_RUNTIME_FIELDS: &[&str] = &[
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
const TEMPERATURE_RUNTIME_FIELDS: &[&str] = &[
    "activeBundle",
    "authorizationSha256",
    "expiresAt",
    "loadedAt",
    "reasonCode",
    "source",
    "state",
];
const RAIN_RUNTIME_FIELDS: &[&str] = &["activeBundle", "loadedAt", "reasonCode", "source", "state"];

const TEMPERATURE_DECISION_FIELDS: &[&str] = &[
    "branch",
    "bundleSha256",
    "correctedTemperatureC",
    "reasonCode",
    "recentErrorStateSha256",
    "sourceForecast",
    "state",
];
const RAIN_DECISION_FIELDS: &[&str] = &[
    "bundleSha256",
    "correctedPrecipitationMm",
    "reasonCode",
    "sourceForecast",
    "state",
];

/// Construct the exact compact disabled runtime while preserving loaded-at presence.
fn disabled_runtime(runtime: &Object, source_order: &[&str]) -> Object {
    source_order
        .iter()
        .filter_map(
            // preserve source order, including omission of an unavailable load clock
            |&name| {
                let value = match name {
                    "loadedAt" => get(runtime, name)?.clone(),
                    "enabledMetrics" => JsonValue::Array(Vec::new()),
                    "reasonCode" => string("admin_disabled"),
                    "state" => string("disabled"),
                    _ => JsonValue::Null,
                };
                Some((name.encode_utf16().collect(), value))
            },
        )
        .collect()
}

/// Preserve the source's spread ordering and append rewritten members in exact order.
fn disable_decision(decision: &mut Object, source_order: &[&str]) {
    // replace known members in place, appending absent members in source order
    for &name in source_order {
        let value = match name {
            "reasonCode" => string("admin_disabled"),
            "state" => string("disabled"),
            _ => JsonValue::Null,
        };
        set(decision, name, value);
    }
}

/// Select an active object bearing the exact state literal.
fn active_object(value: &mut JsonValue) -> Option<&mut Object> {
    let object = as_object_mut(value)?;
    get(object, "state")
        .is_some_and(
            // compare the original active state spelling
            |state| is_string(state, "active"),
        )
        .then_some(object)
}

/// Borrow named members from an ordered generated json object.
fn as_object_mut(value: &mut JsonValue) -> Option<&mut Object> {
    match value {
        JsonValue::Object(object) => Some(object),
        _ => None,
    }
}

/// Compare the original UTF-16 string without allocating a runtime codec.
fn is_string(value: &JsonValue, name: &str) -> bool {
    matches!(value, JsonValue::String(value) if value.iter().copied().eq(name.encode_utf16()))
}

/// Lookup one named field without moving its insertion position.
fn get<'a>(object: &'a Object, name: &str) -> Option<&'a JsonValue> {
    object.iter().find_map(
        // preserve the exact UTF-16 key
        |(key, value)| key.iter().copied().eq(name.encode_utf16()).then_some(value),
    )
}

/// Borrow a named field mutably while keeping the original insertion order.
fn get_mut<'a>(object: &'a mut Object, name: &str) -> Option<&'a mut JsonValue> {
    object.iter_mut().find_map(
        // preserve the exact UTF-16 key
        |(key, value)| key.iter().copied().eq(name.encode_utf16()).then_some(value),
    )
}

/// Replicate assignment order for an existing or newly appended known member.
fn set(object: &mut Object, name: &str, value: JsonValue) {
    match get_mut(object, name) {
        Some(current) => *current = value,
        None => object.push((name.encode_utf16().collect(), value)),
    }
}

/// Set one metric-originated key without changing its earlier insertion position.
fn upsert(object: &mut Object, key: Vec<u16>, value: JsonValue) {
    // preserve the original from-entries order on a repeated key
    if let Some((_, current)) = object.iter_mut().find(
        // retain the exact UTF-16 metric key
        |(name, _)| *name == key,
    ) {
        *current = value;
    } else {
        object.push((key, value));
    }
}

/// Construct one fixed-shape string value for the approved JSON encoder.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Construct one exact fixed-order object from closed source field declarations.
fn object<const N: usize>(fields: [(&str, JsonValue); N]) -> JsonValue {
    JsonValue::Object(object_fields(fields))
}

/// Materialize fixed object fields without implementing a serializer.
fn object_fields<const N: usize>(fields: [(&str, JsonValue); N]) -> Object {
    fields
        .into_iter()
        .map(
            // encode only the original field names, not json syntax
            |(name, value)| (name.encode_utf16().collect(), value),
        )
        .collect()
}
