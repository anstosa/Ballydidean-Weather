use std::error::Error;
use std::fmt::{Display, Formatter};

use crate::{AdmittedAdminMutation, AdmittedAdminOperator};

/// the fixed public property-map center used by placement bounds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PropertySensorMapCenter {
    pub latitude: f64,
    pub longitude: f64,
}

/// the closed persisted property sensor icon set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropertySensorIcon {
    AirQuality,
    Rain,
    Temperature,
    Wind,
}

impl PropertySensorIcon {
    /// return the exact persisted icon wire value.
    pub const fn wire_value(self) -> &'static str {
        // preserve the source store's four icon categories
        match self {
            Self::AirQuality => "air-quality",
            Self::Rain => "rain",
            Self::Temperature => "temperature",
            Self::Wind => "wind",
        }
    }
}

/// one canonical update instant admitted by the owning time boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedPropertySensorUpdateInstant(String);

impl AdmittedPropertySensorUpdateInstant {
    /// admit one canonical UTC instant without implementing a time codec.
    pub fn admitted(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// borrow the canonical instant.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// one validated persisted and public property sensor placement.
#[derive(Debug, Clone, PartialEq)]
pub struct PropertySensorLayoutEntry {
    pub display_name: String,
    pub icon: Option<PropertySensorIcon>,
    pub latitude: f64,
    pub longitude: f64,
    pub sensor_key: String,
    pub updated_at: AdmittedPropertySensorUpdateInstant,
}

/// decoded eligible update fields supplied by the existing JSON adapter.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodedPropertySensorLayoutUpdate {
    pub display_name: Option<String>,
    pub icon: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
}

/// the admitted update-time boundary retained outside layout policy.
pub trait PropertySensorLayoutUpdateInstantSource {
    type Error;

    /// admit the current canonical update instant after the layout read.
    fn admitted_update_instant(
        &mut self,
    ) -> Result<AdmittedPropertySensorUpdateInstant, Self::Error>;
}

/// the exact method allow-list selected by the public or admin route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropertySensorLayoutAllow {
    PublicRead,
    AdminUpdate,
}

impl PropertySensorLayoutAllow {
    /// return the existing handler's exact Allow header value.
    pub const fn header_value(self) -> &'static str {
        // preserve the separate read and update routes
        match self {
            Self::PublicRead => "GET, HEAD",
            Self::AdminUpdate => "PUT",
        }
    }
}

/// one request after URI, body, session, origin, and time admission.
#[derive(Debug, Clone, PartialEq)]
pub enum PropertySensorLayoutRequest<'a> {
    PublicRead {
        omit_body: bool,
    },
    PublicUnsupported,
    AdminReplace {
        mutation: AdmittedAdminMutation<'a>,
        sensor_key: String,
        input: DecodedPropertySensorLayoutUpdate,
    },
    AdminUnsupported {
        operator: &'a AdmittedAdminOperator,
    },
}

/// one typed handler result for the existing response adapter.
#[derive(Debug, Clone, PartialEq)]
pub enum PropertySensorLayoutOutcome {
    ReadSuccess {
        data: Vec<PropertySensorLayoutEntry>,
        omit_body: bool,
    },
    UpdateSuccess {
        data: PropertySensorLayoutEntry,
    },
    InvalidRequest,
    StoreFailure,
    MethodNotAllowed {
        allow: PropertySensorLayoutAllow,
    },
}

impl PropertySensorLayoutOutcome {
    /// return the exact status selected by the Node handlers.
    pub const fn status_code(&self) -> u16 {
        // preserve success and ordinary handler failure categories
        match self {
            Self::ReadSuccess { .. } | Self::UpdateSuccess { .. } => 200,
            Self::InvalidRequest => 400,
            Self::StoreFailure => 500,
            Self::MethodNotAllowed { .. } => 405,
        }
    }

    /// return the source route's response cache policy.
    pub const fn cache_control(&self) -> &'static str {
        // only the public read overrides the default no-store policy
        match self {
            Self::ReadSuccess { .. } => "no-cache",
            _ => "no-store",
        }
    }
}

/// one exact update validation failure before any store operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropertySensorLayoutValidationError {
    InvalidSensorKey,
    InvalidDisplayName,
    InvalidIcon,
    InvalidCoordinates,
    OutsidePropertyBounds,
}

impl Display for PropertySensorLayoutValidationError {
    /// render the owning store's exact validation text.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        // preserve the source store's validation categories
        match self {
            Self::InvalidSensorKey => formatter.write_str("sensor key is invalid"),
            Self::InvalidDisplayName => {
                formatter.write_str("displayName must be between 1 and 80 characters")
            }
            Self::InvalidIcon => formatter.write_str("sensor icon is invalid"),
            Self::InvalidCoordinates => {
                formatter.write_str("sensor position coordinates are invalid")
            }
            Self::OutsidePropertyBounds => {
                formatter.write_str("sensor position must be within the property map bounds")
            }
        }
    }
}

impl Error for PropertySensorLayoutValidationError {}

/// atomic property sensor storage injected by the owning runtime.
pub trait PropertySensorLayoutStore {
    type Error;

    /// read the validated public layout or the missing-layout default.
    fn read_layout(&mut self) -> Result<Vec<PropertySensorLayoutEntry>, Self::Error>;

    /// replace the complete ordered layout atomically.
    fn replace_layout_atomically(
        &mut self,
        layout: Vec<PropertySensorLayoutEntry>,
    ) -> Result<(), Self::Error>;
}

/// an injected no-filesystem layout store for the Rust edge runtime.
#[derive(Debug, Clone, PartialEq)]
pub struct InMemoryPropertySensorLayoutStore {
    layout: Option<Vec<PropertySensorLayoutEntry>>,
}

impl InMemoryPropertySensorLayoutStore {
    /// create the missing first-run layout state.
    pub const fn missing() -> Self {
        Self { layout: None }
    }

    /// create one admitted persisted layout state.
    pub fn persisted(layout: Vec<PropertySensorLayoutEntry>) -> Self {
        Self {
            layout: Some(layout),
        }
    }
}

impl PropertySensorLayoutStore for InMemoryPropertySensorLayoutStore {
    type Error = std::convert::Infallible;

    /// return the missing default or the bounded persisted prefix.
    fn read_layout(&mut self) -> Result<Vec<PropertySensorLayoutEntry>, Self::Error> {
        Ok(self
            .layout
            .as_deref()
            .unwrap_or_default()
            .iter()
            .take(64)
            .cloned()
            .collect())
    }

    /// atomically replace the complete in-memory layout.
    fn replace_layout_atomically(
        &mut self,
        layout: Vec<PropertySensorLayoutEntry>,
    ) -> Result<(), Self::Error> {
        self.layout = Some(layout);
        Ok(())
    }
}

/// execute the property sensor layout workflow over one injected store.
pub fn handle_property_sensor_layout<Store, UpdateInstants>(
    center: PropertySensorMapCenter,
    request: PropertySensorLayoutRequest<'_>,
    store: &mut Store,
    update_instants: &mut UpdateInstants,
) -> PropertySensorLayoutOutcome
where
    Store: PropertySensorLayoutStore,
    UpdateInstants: PropertySensorLayoutUpdateInstantSource,
{
    // preserve public read, admin replacement, and route method modes
    match request {
        PropertySensorLayoutRequest::PublicRead { omit_body } => match store.read_layout() {
            Ok(data) => PropertySensorLayoutOutcome::ReadSuccess { data, omit_body },
            Err(_) => PropertySensorLayoutOutcome::StoreFailure,
        },
        PropertySensorLayoutRequest::PublicUnsupported => {
            PropertySensorLayoutOutcome::MethodNotAllowed {
                allow: PropertySensorLayoutAllow::PublicRead,
            }
        }
        PropertySensorLayoutRequest::AdminReplace {
            mutation,
            sensor_key,
            input,
        } => {
            let _verified_operator = mutation;
            let validated =
                match validate_property_sensor_layout_update_fields(center, &sensor_key, input) {
                    Ok(validated) => validated,
                    Err(_) => return PropertySensorLayoutOutcome::InvalidRequest,
                };
            let mut next = match store.read_layout() {
                Ok(layout) => layout,
                Err(_) => return PropertySensorLayoutOutcome::StoreFailure,
            };
            let updated_at = match update_instants.admitted_update_instant() {
                Ok(updated_at) => updated_at,
                Err(_) => return PropertySensorLayoutOutcome::StoreFailure,
            };
            let saved = validated.into_entry(updated_at);
            next.retain(|entry| entry.sensor_key != saved.sensor_key);
            next.push(saved.clone());
            // keep deterministic public responses and persisted state
            next.sort_by(|left, right| left.sensor_key.cmp(&right.sensor_key));

            // expose the saved entry only after atomic layout replacement
            match store.replace_layout_atomically(next) {
                Ok(()) => PropertySensorLayoutOutcome::UpdateSuccess { data: saved },
                Err(_) => PropertySensorLayoutOutcome::StoreFailure,
            }
        }
        PropertySensorLayoutRequest::AdminUnsupported { operator } => {
            let _verified_operator = operator;
            PropertySensorLayoutOutcome::MethodNotAllowed {
                allow: PropertySensorLayoutAllow::AdminUpdate,
            }
        }
    }
}

/// validate and build one exact saved placement.
pub fn validate_property_sensor_layout_update(
    center: PropertySensorMapCenter,
    sensor_key: &str,
    input: DecodedPropertySensorLayoutUpdate,
    updated_at: AdmittedPropertySensorUpdateInstant,
) -> Result<PropertySensorLayoutEntry, PropertySensorLayoutValidationError> {
    validate_property_sensor_layout_update_fields(center, sensor_key, input)
        .map(|validated| validated.into_entry(updated_at))
}

#[derive(Debug, Clone, PartialEq)]
struct ValidatedPropertySensorLayoutUpdate {
    display_name: String,
    icon: PropertySensorIcon,
    latitude: f64,
    longitude: f64,
    sensor_key: String,
}

impl ValidatedPropertySensorLayoutUpdate {
    /// attach the admitted post-read update instant to one validated update.
    fn into_entry(
        self,
        updated_at: AdmittedPropertySensorUpdateInstant,
    ) -> PropertySensorLayoutEntry {
        PropertySensorLayoutEntry {
            display_name: self.display_name,
            icon: Some(self.icon),
            latitude: self.latitude,
            longitude: self.longitude,
            sensor_key: self.sensor_key,
            updated_at,
        }
    }
}

/// validate eligible fields without observing the update clock.
fn validate_property_sensor_layout_update_fields(
    center: PropertySensorMapCenter,
    sensor_key: &str,
    input: DecodedPropertySensorLayoutUpdate,
) -> Result<ValidatedPropertySensorLayoutUpdate, PropertySensorLayoutValidationError> {
    let sensor_key = validate_sensor_key(sensor_key)?;
    let display_name = validate_display_name(input.display_name)?;
    let icon = validate_sensor_icon(input.icon)?;
    let (latitude, longitude) = validate_coordinate(input.latitude, input.longitude)?;

    // constrain placements to the configured property vicinity
    if (latitude - center.latitude).abs() > 0.025 || (longitude - center.longitude).abs() > 0.04 {
        return Err(PropertySensorLayoutValidationError::OutsidePropertyBounds);
    }

    Ok(ValidatedPropertySensorLayoutUpdate {
        display_name,
        icon,
        latitude,
        longitude,
        sensor_key,
    })
}

/// validate one stable lowercase sensor identity without a regex dependency.
fn validate_sensor_key(value: &str) -> Result<String, PropertySensorLayoutValidationError> {
    let valid_length = !value.is_empty() && value.encode_utf16().count() <= 64;
    let valid_segments = value.split('-').all(|segment| {
        !segment.is_empty()
            && segment
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
    });

    // reject path-like, non-ascii, uppercase, empty, or unbounded keys
    if !valid_length || !valid_segments {
        return Err(PropertySensorLayoutValidationError::InvalidSensorKey);
    }

    Ok(value.to_owned())
}

/// validate one trimmed user-facing sensor name.
fn validate_display_name(
    value: Option<String>,
) -> Result<String, PropertySensorLayoutValidationError> {
    let value = value.ok_or(PropertySensorLayoutValidationError::InvalidDisplayName)?;
    let trimmed = value.trim();
    let valid_length = (1..=80).contains(&trimmed.encode_utf16().count());
    let has_control = value
        .chars()
        .any(|character| matches!(character, '\u{0000}'..='\u{001f}' | '\u{007f}'));

    // require readable bounded text without controls
    if !valid_length || has_control {
        return Err(PropertySensorLayoutValidationError::InvalidDisplayName);
    }

    Ok(trimmed.to_owned())
}

/// validate one explicit icon category.
fn validate_sensor_icon(
    value: Option<String>,
) -> Result<PropertySensorIcon, PropertySensorLayoutValidationError> {
    // preserve the source store's closed icon set
    match value.as_deref() {
        Some("air-quality") => Ok(PropertySensorIcon::AirQuality),
        Some("rain") => Ok(PropertySensorIcon::Rain),
        Some("temperature") => Ok(PropertySensorIcon::Temperature),
        Some("wind") => Ok(PropertySensorIcon::Wind),
        _ => Err(PropertySensorLayoutValidationError::InvalidIcon),
    }
}

/// validate one finite geographic coordinate pair.
fn validate_coordinate(
    latitude: Option<f64>,
    longitude: Option<f64>,
) -> Result<(f64, f64), PropertySensorLayoutValidationError> {
    let latitude = latitude.ok_or(PropertySensorLayoutValidationError::InvalidCoordinates)?;
    let longitude = longitude.ok_or(PropertySensorLayoutValidationError::InvalidCoordinates)?;

    // enforce the source store's geographic coordinate ranges
    if !latitude.is_finite()
        || !(-85.0..=85.0).contains(&latitude)
        || !longitude.is_finite()
        || !(-180.0..=180.0).contains(&longitude)
    {
        return Err(PropertySensorLayoutValidationError::InvalidCoordinates);
    }

    Ok((latitude, longitude))
}
