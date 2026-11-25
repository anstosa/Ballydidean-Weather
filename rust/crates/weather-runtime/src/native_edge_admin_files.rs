//! Native admin settings and sensor placement file owners sharing the existing auth evidence.

use crate::{
    NativeWeatherAdminAuthState, NativeWirePrimitives, maintenance_canonical_json::field,
    system_worker_health_milliseconds,
};
use std::{
    fmt::{Display, Formatter},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
use weather_contracts::JsonValue;
use weather_edge::{
    AdmittedForecastAdjustmentSettingsInitialization, AdmittedForecastAdjustmentSettingsJson,
    AdmittedPropertySensorLayoutJson, AdmittedPropertySensorUpdateInstant, ArchiveWirePrimitives,
    DEFAULT_FORECAST_ADJUSTMENT_SETTINGS, DISABLED_FORECAST_ADJUSTMENT_SETTINGS,
    DecodedForecastAdjustmentSettings, DecodedPropertySensorLayoutUpdate, EdgeWireJsonEncoder,
    FileForecastAdjustmentSettingsStore, FilePropertySensorLayoutStore, ForecastAdjustmentSettings,
    ForecastAdjustmentSettingsDiagnostic, ForecastAdjustmentSettingsEnvelope,
    ForecastAdjustmentSettingsInitializationEvidence,
    ForecastAdjustmentSettingsInitializationEvidenceReader, ForecastAdjustmentSettingsJsonAdapter,
    ForecastAdjustmentSettingsStatus, ForecastAdjustmentSettingsStore,
    ForecastAdjustmentSettingsTemporaryPathSource, PropertySensorLayoutEntry,
    PropertySensorLayoutJsonAdapter, PropertySensorLayoutTemporaryPathSource,
    PropertySensorLayoutUpdateInstantSource, PropertySensorMapCenter, PublicAdjustmentSnapshot,
    RevisionArchiveCanonicalCodec, WeatherAdminAuthStatePort,
    validate_forecast_adjustment_settings, validate_property_sensor_layout_update,
};

/// One invalid native local JSON or file-state value without private content in diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeEdgeAdminFileError;
impl Display for NativeEdgeAdminFileError {
    /// Retain a stable ordinary state-file diagnostic without embedding document values.
    fn fmt(&self, out: &mut Formatter<'_>) -> std::fmt::Result {
        out.write_str("weather admin state is invalid or unavailable")
    }
}
impl std::error::Error for NativeEdgeAdminFileError {}

/// Native file JSON owner for the exact predecessor schemas.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeEdgeAdminJson;

/// Decode Node's UTF-8 file text boundary into the retained ordered JSON value.
fn document(bytes: &[u8]) -> Result<JsonValue, NativeEdgeAdminFileError> {
    NativeWirePrimitives
        .decode_json(String::from_utf8_lossy(bytes).as_bytes())
        .ok_or(NativeEdgeAdminFileError)
}

/// Produce one named ordered JSON member.
fn member(name: &str, value: JsonValue) -> (Vec<u16>, JsonValue) {
    (name.encode_utf16().collect(), value)
}

/// Produce one generated UTF-16 string member value.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Decode one field without changing the original type.
fn text(value: &JsonValue, name: &str) -> Option<String> {
    let JsonValue::String(units) = field(value, name)? else {
        return None;
    };
    String::from_utf16(units).ok()
}

/// Decode one numeric field without coercion.
fn number(value: &JsonValue, name: &str) -> Option<f64> {
    let JsonValue::Number(number) = field(value, name)? else {
        return None;
    };
    Some(*number)
}

/// Decode one boolean field without coercion.
fn boolean(value: &JsonValue, name: &str) -> Option<bool> {
    let JsonValue::Boolean(value) = field(value, name)? else {
        return None;
    };
    Some(*value)
}

impl ForecastAdjustmentSettingsJsonAdapter for NativeEdgeAdminJson {
    type Error = NativeEdgeAdminFileError;

    /// Delegate the fixed four-field settings validation to the retained policy owner.
    fn decode(&mut self, content: &[u8]) -> Result<ForecastAdjustmentSettings, Self::Error> {
        let value = document(content)?;
        let JsonValue::Object(fields) = &value else {
            return Err(NativeEdgeAdminFileError);
        };
        validate_forecast_adjustment_settings(DecodedForecastAdjustmentSettings {
            field_count: fields.len(),
            version: number(&value, "version")
                .filter(|value| *value == 1.0)
                .map(|_| 1),
            temperature: boolean(&value, "temperature"),
            wind: boolean(&value, "wind"),
            rain: boolean(&value, "rain"),
        })
        .map_err(|_| NativeEdgeAdminFileError)
    }

    /// Preserve the predecessor's fixed compact field order and one final line feed.
    fn encode(
        &mut self,
        settings: ForecastAdjustmentSettings,
    ) -> Result<AdmittedForecastAdjustmentSettingsJson, Self::Error> {
        let body = settings_value(settings)?;
        NativeWirePrimitives
            .encode_ordered_json(&body)
            .map(AdmittedForecastAdjustmentSettingsJson::admitted)
            .ok_or(NativeEdgeAdminFileError)
    }
}

impl PropertySensorLayoutJsonAdapter for NativeEdgeAdminJson {
    type Error = NativeEdgeAdminFileError;

    /// Preserve the original bounded persisted layout array and nullable legacy icons.
    fn decode(&mut self, content: &[u8]) -> Result<Vec<PropertySensorLayoutEntry>, Self::Error> {
        let value = document(content)?;
        if number(&value, "version") != Some(1.0) {
            return Err(NativeEdgeAdminFileError);
        }
        let Some(JsonValue::Array(entries)) = field(&value, "sensors") else {
            return Err(NativeEdgeAdminFileError);
        };
        entries.iter().take(64).map(decode_layout_entry).collect()
    }

    /// Persist exact ordered source geometry and nullable icon values with the original newline.
    fn encode(
        &mut self,
        layout: &[PropertySensorLayoutEntry],
    ) -> Result<AdmittedPropertySensorLayoutJson, Self::Error> {
        let value = JsonValue::Object(vec![
            member("sensors", layout_value(layout)),
            member("version", JsonValue::Number(1.0)),
        ]);
        NativeWirePrimitives
            .encode_ordered_json(&value)
            .map(AdmittedPropertySensorLayoutJson::admitted)
            .ok_or(NativeEdgeAdminFileError)
    }
}

/// Retain the fixed response and persisted settings member order.
fn settings_value(
    settings: ForecastAdjustmentSettings,
) -> Result<JsonValue, NativeEdgeAdminFileError> {
    // the canonical source settings are v1 only
    if settings.version != 1 {
        return Err(NativeEdgeAdminFileError);
    }
    Ok(JsonValue::Object(vec![
        member("version", JsonValue::Number(1.0)),
        member("temperature", JsonValue::Boolean(settings.temperature)),
        member("wind", JsonValue::Boolean(settings.wind)),
        member("rain", JsonValue::Boolean(settings.rain)),
    ]))
}

/// Retain the source layout geometry and explicit null legacy icon.
fn layout_entry_value(entry: &PropertySensorLayoutEntry) -> JsonValue {
    JsonValue::Object(vec![
        member("displayName", string(&entry.display_name)),
        member(
            "icon",
            entry
                .icon
                .map_or(JsonValue::Null, |icon| string(icon.wire_value())),
        ),
        member("latitude", JsonValue::Number(entry.latitude)),
        member("longitude", JsonValue::Number(entry.longitude)),
        member("sensorKey", string(&entry.sensor_key)),
        member("updatedAt", string(entry.updated_at.as_str())),
    ])
}

/// Materialize the ordered entries without re-parsing persistent bytes.
fn layout_value(layout: &[PropertySensorLayoutEntry]) -> JsonValue {
    JsonValue::Array(layout.iter().map(layout_entry_value).collect())
}

/// Encode the standard response envelope with the source's single trailing LF.
fn admin_wire(value: JsonValue) -> Result<Vec<u8>, NativeEdgeAdminFileError> {
    NativeWirePrimitives
        .encode_ordered_json(&value)
        .ok_or(NativeEdgeAdminFileError)
}

impl EdgeWireJsonEncoder for NativeEdgeAdminJson {
    type Error = NativeEdgeAdminFileError;

    /// Encode the data switch snapshot and include only a selected administrator diagnostic.
    fn encode_adjustment_settings(
        &mut self,
        envelope: &ForecastAdjustmentSettingsEnvelope,
    ) -> Result<Vec<u8>, Self::Error> {
        let mut fields = vec![member("data", settings_value(envelope.data)?)];
        // the source omits the error when the route does not disclose a diagnostic
        if let Some(error) = envelope.error {
            fields.push(member("error", string(error.wire_value())));
        }
        admin_wire(JsonValue::Object(fields))
    }

    /// Encode the public property layout envelope.
    fn encode_property_sensor_layout(
        &mut self,
        entries: &[PropertySensorLayoutEntry],
    ) -> Result<Vec<u8>, Self::Error> {
        admin_wire(JsonValue::Object(vec![member(
            "data",
            layout_value(entries),
        )]))
    }

    /// Encode only the updated persisted placement.
    fn encode_property_sensor_layout_entry(
        &mut self,
        entry: &PropertySensorLayoutEntry,
    ) -> Result<Vec<u8>, Self::Error> {
        admin_wire(JsonValue::Object(vec![member(
            "data",
            layout_entry_value(entry),
        )]))
    }

    /// Emit the original no-LF viewer context representation.
    fn encode_viewer_context(&mut self, home_network: bool) -> Result<Vec<u8>, Self::Error> {
        let mut body = admin_wire(JsonValue::Object(vec![member(
            "data",
            JsonValue::Object(vec![member(
                "homeNetwork",
                JsonValue::Boolean(home_network),
            )]),
        )]))?;
        // the viewer endpoint is emitted directly rather than through sendJson
        if body.pop() != Some(b'\n') {
            return Err(NativeEdgeAdminFileError);
        }
        Ok(body)
    }
}

/// Validate one persisted entry without requiring a property-local mutation bound on already saved coordinates.
fn decode_layout_entry(
    value: &JsonValue,
) -> Result<PropertySensorLayoutEntry, NativeEdgeAdminFileError> {
    let latitude = number(value, "latitude").ok_or(NativeEdgeAdminFileError)?;
    let longitude = number(value, "longitude").ok_or(NativeEdgeAdminFileError)?;
    let sensor_key = text(value, "sensorKey").ok_or(NativeEdgeAdminFileError)?;
    let updated_at = text(value, "updatedAt").ok_or(NativeEdgeAdminFileError)?;
    NativeWirePrimitives
        .canonical_utc_milliseconds(&updated_at)
        .ok_or(NativeEdgeAdminFileError)?;
    // the source accepts an absent or null icon only in persisted legacy entries
    let icon = match field(value, "icon") {
        None | Some(JsonValue::Null) => None,
        Some(JsonValue::String(units)) => {
            Some(String::from_utf16(units).map_err(|_| NativeEdgeAdminFileError)?)
        }
        _ => return Err(NativeEdgeAdminFileError),
    };
    // reuse the existing key, name, coordinate and icon checks without adding a new placement restriction
    let mut entry = validate_property_sensor_layout_update(
        PropertySensorMapCenter {
            latitude,
            longitude,
        },
        &sensor_key,
        DecodedPropertySensorLayoutUpdate {
            display_name: text(value, "displayName"),
            icon: Some(icon.clone().unwrap_or_else(|| "rain".to_owned())),
            latitude: Some(latitude),
            longitude: Some(longitude),
        },
        AdmittedPropertySensorUpdateInstant::admitted(updated_at),
    )
    .map_err(|_| NativeEdgeAdminFileError)?;
    if icon.is_none() {
        entry.icon = None;
    }
    Ok(entry)
}

/// Same-directory unpredictable temporary names backed by existing process entropy and a unique sequence.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeEdgeAdminTemporaryPaths;
static TEMPORARY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Produce a unique private replacement path without creating or overwriting it.
fn temporary_path(target: &Path) -> PathBuf {
    let sequence = TEMPORARY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let mut entropy = [0_u8; 8];
    let _ = getrandom::fill(&mut entropy);
    let suffix = format!(
        ".{}.{sequence}.{:016x}.tmp",
        std::process::id(),
        u64::from_le_bytes(entropy)
    );
    let mut path = target.as_os_str().to_os_string();
    path.push(suffix);
    PathBuf::from(path)
}

impl ForecastAdjustmentSettingsTemporaryPathSource for NativeEdgeAdminTemporaryPaths {
    /// Retain atomic replacement on the configured settings filesystem.
    fn next_temporary_path(&mut self, target: &Path) -> PathBuf {
        temporary_path(target)
    }
}
impl PropertySensorLayoutTemporaryPathSource for NativeEdgeAdminTemporaryPaths {
    /// Retain atomic replacement on the configured placement filesystem.
    fn next_temporary_path(&mut self, target: &Path) -> PathBuf {
        temporary_path(target)
    }
}

/// Read the same persisted authentication owner before any first settings initialization.
#[derive(Debug)]
pub struct NativeEdgeAdminInitializationEvidence(NativeWeatherAdminAuthState);
impl ForecastAdjustmentSettingsInitializationEvidenceReader
    for NativeEdgeAdminInitializationEvidence
{
    /// Admit initialization only from the actual valid configured auth record.
    fn read_initialization_evidence(&mut self) -> ForecastAdjustmentSettingsInitializationEvidence {
        match self.0.read_auth_state() {
            Ok(Some(_)) => ForecastAdjustmentSettingsInitializationEvidence::Admitted(
                AdmittedForecastAdjustmentSettingsInitialization::admitted(),
            ),
            Ok(None) => ForecastAdjustmentSettingsInitializationEvidence::Missing,
            Err(_) => ForecastAdjustmentSettingsInitializationEvidence::Invalid,
        }
    }
}

/// Shared process settings store used by public projections and verified admin requests.
pub type NativeEdgeSettingsFile = FileForecastAdjustmentSettingsStore<
    NativeEdgeAdminJson,
    NativeEdgeAdminTemporaryPaths,
    NativeEdgeAdminInitializationEvidence,
>;

/// One cloneable handle over the exact same native persisted settings state.
#[derive(Clone)]
pub struct NativeSharedAdjustmentSettings(Arc<Mutex<NativeEdgeSettingsFile>>);
impl NativeSharedAdjustmentSettings {
    /// Construct one store with the source's initialization default and live auth evidence.
    pub fn new(
        settings_path: PathBuf,
        auth_path: PathBuf,
    ) -> Result<Self, NativeEdgeAdminFileError> {
        let default = ForecastAdjustmentSettingsJsonAdapter::encode(
            &mut NativeEdgeAdminJson,
            DEFAULT_FORECAST_ADJUSTMENT_SETTINGS,
        )?;
        Ok(Self(Arc::new(Mutex::new(
            FileForecastAdjustmentSettingsStore::new(
                settings_path,
                default,
                NativeEdgeAdminInitializationEvidence(NativeWeatherAdminAuthState::new(auth_path)),
                NativeEdgeAdminJson,
                NativeEdgeAdminTemporaryPaths,
            ),
        ))))
    }
}
impl ForecastAdjustmentSettingsStore for NativeSharedAdjustmentSettings {
    type Error = NativeEdgeAdminFileError;
    /// Read one shared snapshot with the original unavailable fallback on store failure.
    fn read_status(&mut self) -> ForecastAdjustmentSettingsStatus {
        self.0.lock().map_or(
            ForecastAdjustmentSettingsStatus {
                settings: DISABLED_FORECAST_ADJUSTMENT_SETTINGS,
                error: Some(ForecastAdjustmentSettingsDiagnostic::Unavailable),
            },
            |mut store| store.read_status(),
        )
    }
    /// Persist one complete admin snapshot through the same shared process store.
    fn replace_atomically(
        &mut self,
        settings: ForecastAdjustmentSettings,
    ) -> Result<ForecastAdjustmentSettings, Self::Error> {
        self.0
            .lock()
            .map_err(|_| NativeEdgeAdminFileError)?
            .replace_atomically(settings)
            .map_err(|_| NativeEdgeAdminFileError)
    }
}
impl PublicAdjustmentSnapshot for NativeSharedAdjustmentSettings {
    /// Snapshot actual persisted adjustment controls before the upstream API fetch.
    fn read_settings(&self) -> ForecastAdjustmentSettings {
        self.clone().read_status().settings
    }
}

/// Concrete native placement file owner used by the edge's public and admin route policy.
pub type NativeEdgeLayoutFile =
    FilePropertySensorLayoutStore<NativeEdgeAdminJson, NativeEdgeAdminTemporaryPaths>;

/// Construct the existing native placement file owner without reading or creating its target.
pub fn native_edge_layout_file(path: PathBuf) -> NativeEdgeLayoutFile {
    FilePropertySensorLayoutStore::new(path, NativeEdgeAdminJson, NativeEdgeAdminTemporaryPaths)
}

/// Actual update-time owner shared by the placement mutation route.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeEdgeAdminUpdateClock;
impl PropertySensorLayoutUpdateInstantSource for NativeEdgeAdminUpdateClock {
    type Error = NativeEdgeAdminFileError;
    /// Capture one process update instant after the existing layout read.
    fn admitted_update_instant(
        &mut self,
    ) -> Result<AdmittedPropertySensorUpdateInstant, Self::Error> {
        system_worker_health_milliseconds()
            .and_then(|value| NativeWirePrimitives.format_utc_milliseconds(value))
            .map(AdmittedPropertySensorUpdateInstant::admitted)
            .ok_or(NativeEdgeAdminFileError)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use weather_edge::PropertySensorLayoutStore;

    /// Exercise actual shared native file state over a bounded temporary fixture directory.
    #[test]
    fn native_edge_admin_files_share_persisted_settings_and_legacy_layout() {
        let root = temporary_path(&std::env::temp_dir().join("weather-native-edge-admin"));
        fs::create_dir(&root).unwrap();
        let auth = root.join("auth.json");
        let settings_path = root.join("forecast-adjustment-settings.json");
        let layout_path = root.join("layout.json");
        let mut settings =
            NativeSharedAdjustmentSettings::new(settings_path.clone(), auth.clone()).unwrap();
        assert_eq!(
            settings.read_settings(),
            DISABLED_FORECAST_ADJUSTMENT_SETTINGS
        );
        let hash = NativeWirePrimitives
            .encode_standard_base64(&[0; 64])
            .unwrap();
        let salt = NativeWirePrimitives
            .encode_standard_base64(&[0; 16])
            .unwrap();
        fs::write(
            &auth,
            format!(
                "{{\"hash\":\"{hash}\",\"salt\":\"{salt}\",\"username\":\"admin\",\"version\":1}}\n"
            ),
        )
        .unwrap();
        assert_eq!(
            settings.read_settings(),
            DEFAULT_FORECAST_ADJUSTMENT_SETTINGS
        );
        let shared = settings.clone();
        settings
            .replace_atomically(DISABLED_FORECAST_ADJUSTMENT_SETTINGS)
            .unwrap();
        assert_eq!(
            shared.read_settings(),
            DISABLED_FORECAST_ADJUSTMENT_SETTINGS
        );
        assert_eq!(
            fs::read(&settings_path).unwrap(),
            b"{\"version\":1,\"temperature\":false,\"wind\":false,\"rain\":false}\n"
        );
        let mut layout = native_edge_layout_file(layout_path.clone());
        fs::write(&layout_path,b"{\"version\":1,\"sensors\":[{\"displayName\":\" Fixture \",\"latitude\":37,\"longitude\":-122,\"sensorKey\":\"fixture-1\",\"updatedAt\":\"2026-11-24T00:00:00.000Z\"}]}\n").unwrap();
        let rows = layout.read_layout().unwrap();
        assert_eq!(rows[0].display_name, "Fixture");
        assert!(rows[0].icon.is_none());
        layout.replace_layout_atomically(rows.clone()).unwrap();
        assert_eq!(layout.read_layout().unwrap(), rows);
        let persisted = fs::read(&layout_path).unwrap();
        assert!(persisted.starts_with(b"{\"sensors\":["));
        assert!(persisted.ends_with(b"],\"version\":1}\n"));
        let mut encoder = NativeEdgeAdminJson;
        assert_eq!(
            encoder.encode_viewer_context(true).unwrap(),
            b"{\"data\":{\"homeNetwork\":true}}"
        );
        assert_eq!(encoder.encode_adjustment_settings(&ForecastAdjustmentSettingsEnvelope {
            data: DEFAULT_FORECAST_ADJUSTMENT_SETTINGS,
            error: Some(ForecastAdjustmentSettingsDiagnostic::Unavailable),
        }).unwrap(), b"{\"data\":{\"version\":1,\"temperature\":true,\"wind\":true,\"rain\":true},\"error\":\"adjustment_settings_unavailable\"}\n");
        fs::remove_dir_all(root).unwrap();
    }
}
