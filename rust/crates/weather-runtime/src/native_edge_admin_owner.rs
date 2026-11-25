//! Retained native administrator state and its complete edge-dispatch composition boundary.

use std::fmt::{self, Display, Formatter};
use std::path::{Path, PathBuf};

use weather_edge::{
    AdminLoginPageRenderer, EdgeStartupConfiguration, ForecastAdjustmentScorecardClock,
    ForecastAdjustmentScorecardEncoder, ForecastAdjustmentScorecardStore, ForecastMapSite,
    ForecastRecordedProductsRead, PropertySensorLayoutOutcome, PropertySensorLayoutRequest,
    PropertySensorMapCenter, PublicApiProxyClient, PublicStaticAssetRead,
    PublicStaticTemplateRenderer, WeatherAdminSessionApplication, WeatherAdminStore,
    WeatherEdgeDispatchOutcome, WeatherEdgeDispatchPorts, WeatherEdgeDispatchRequest,
    WidgetForecastClient, WidgetForecastReceivedAtSource, WidgetForecastSnapshotProjector,
    XweatherMapTileCache, dispatch_weather_edge_request, handle_property_sensor_layout,
};

use crate::{
    NativeEdgeAdminFileError, NativeEdgeAdminUpdateClock, NativeEdgeLayoutFile,
    NativeSharedAdjustmentSettings, NativeWeatherAdminAuthState, NativeWeatherAdminSessionClock,
    NativeWeatherAdminSessionMaterial, native_edge_layout_file,
};

const FALLBACK_PROPERTY_SENSOR_CENTER: PropertySensorMapCenter = PropertySensorMapCenter {
    latitude: 47.95043,
    longitude: -122.42797,
};
const ADJUSTMENT_SETTINGS_FILE_NAME: &str = "forecast-adjustment-settings.json";

/// Construction failures before the retained administrator owner is available to listeners.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeEdgeAdminOwnerError {
    InvalidPath,
    InvalidPropertySensorCenter,
    Settings(NativeEdgeAdminFileError),
}

impl Display for NativeEdgeAdminOwnerError {
    /// Describe only the failed owner boundary without including configured paths or state.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        // retain stable startup categories without leaking deployment layout
        match self {
            Self::InvalidPath => formatter.write_str("weather admin path is invalid"),
            Self::InvalidPropertySensorCenter => {
                formatter.write_str("weather admin property sensor center is invalid")
            }
            Self::Settings(error) => Display::fmt(error, formatter),
        }
    }
}

impl std::error::Error for NativeEdgeAdminOwnerError {
    /// Retain the settings construction source without manufacturing path details.
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        // only the file adapter exposes a nested ordinary error
        match self {
            Self::Settings(error) => Some(error),
            Self::InvalidPath | Self::InvalidPropertySensorCenter => None,
        }
    }
}

impl From<NativeEdgeAdminFileError> for NativeEdgeAdminOwnerError {
    /// Preserve the file owner's bounded construction failure.
    fn from(value: NativeEdgeAdminFileError) -> Self {
        Self::Settings(value)
    }
}

/// Process-retained native state shared by administrator sessions, settings, and layout routes.
pub struct NativeEdgeAdminOwner {
    auth: NativeWeatherAdminAuthState,
    clock: NativeWeatherAdminSessionClock,
    layout: NativeEdgeLayoutFile,
    layout_update_instants: NativeEdgeAdminUpdateClock,
    material: NativeWeatherAdminSessionMaterial,
    property_sensor_center: PropertySensorMapCenter,
    sessions: WeatherAdminStore,
    settings: NativeSharedAdjustmentSettings,
}

impl NativeEdgeAdminOwner {
    /// Construct the exact predecessor owner from admitted startup paths and optional site data.
    pub fn from_configuration<Origin>(
        configuration: &EdgeStartupConfiguration<Origin>,
        site: Option<&ForecastMapSite>,
    ) -> Result<Self, NativeEdgeAdminOwnerError> {
        let center = site
            .filter(|site| valid_property_sensor_center(site.latitude, site.longitude))
            .map_or(FALLBACK_PROPERTY_SENSOR_CENTER, |site| {
                PropertySensorMapCenter {
                    latitude: site.latitude,
                    longitude: site.longitude,
                }
            });
        let settings_path =
            native_edge_adjustment_settings_path(&configuration.property_sensor_layout_path);
        Self::new(
            configuration.admin_auth_path.clone(),
            settings_path,
            configuration.property_sensor_layout_path.clone(),
            center,
        )
    }

    /// Bind all persistent and process-local administrator state without reading route data.
    pub fn new(
        auth_path: PathBuf,
        settings_path: PathBuf,
        layout_path: PathBuf,
        property_sensor_center: PropertySensorMapCenter,
    ) -> Result<Self, NativeEdgeAdminOwnerError> {
        // reject empty paths at the same construction boundary as the predecessor store
        if [&auth_path, &settings_path, &layout_path]
            .iter()
            .any(|path| path.as_os_str().is_empty())
        {
            return Err(NativeEdgeAdminOwnerError::InvalidPath);
        }
        // reject non-finite or out-of-range placement centers before retaining route state
        if !valid_property_sensor_center(
            property_sensor_center.latitude,
            property_sensor_center.longitude,
        ) {
            return Err(NativeEdgeAdminOwnerError::InvalidPropertySensorCenter);
        }
        let settings = NativeSharedAdjustmentSettings::new(settings_path, auth_path.clone())?;
        Ok(Self {
            auth: NativeWeatherAdminAuthState::new(auth_path),
            clock: NativeWeatherAdminSessionClock::new(),
            layout: native_edge_layout_file(layout_path),
            layout_update_instants: NativeEdgeAdminUpdateClock,
            material: NativeWeatherAdminSessionMaterial::new(),
            property_sensor_center,
            sessions: WeatherAdminStore::new(),
            settings,
        })
    }

    /// Borrow the exact shared settings handle used by public adjustment projections.
    pub fn shared_adjustment_settings(&self) -> NativeSharedAdjustmentSettings {
        self.settings.clone()
    }

    /// Borrow the persisted auth owner for request-admission work outside route policy.
    pub const fn auth_state_mut(&mut self) -> &mut NativeWeatherAdminAuthState {
        &mut self.auth
    }

    /// Borrow the adjustment settings owner for a separately composed route.
    pub const fn settings_mut(&mut self) -> &mut NativeSharedAdjustmentSettings {
        &mut self.settings
    }

    /// Execute one public or admitted administrator layout route over retained file state.
    pub fn property_sensor_layout(
        &mut self,
        request: PropertySensorLayoutRequest<'_>,
    ) -> PropertySensorLayoutOutcome {
        handle_property_sensor_layout(
            self.property_sensor_center,
            request,
            &mut self.layout,
            &mut self.layout_update_instants,
        )
    }

    /// Bind one request-scoped session application to the retained process session registry.
    pub fn session_application(
        &mut self,
    ) -> WeatherAdminSessionApplication<
        '_,
        NativeWeatherAdminAuthState,
        NativeWeatherAdminSessionClock,
        NativeWeatherAdminSessionMaterial,
    > {
        WeatherAdminSessionApplication::new(
            &mut self.sessions,
            &mut self.auth,
            &mut self.clock,
            &mut self.material,
        )
    }

    /// Dispatch through real external route owners while supplying every retained admin port.
    pub fn dispatch<
        Assets,
        Renderer,
        AdminLoginRenderer,
        Api,
        WidgetClient,
        WidgetClock,
        WidgetProjector,
        Recorded,
        MapCache,
        ScorecardClock,
        ScorecardStore,
        ScorecardEncoder,
    >(
        &mut self,
        request: WeatherEdgeDispatchRequest<'_>,
        routes: NativeEdgeRouteOwners<
            '_,
            Assets,
            Renderer,
            AdminLoginRenderer,
            Api,
            WidgetClient,
            WidgetClock,
            WidgetProjector,
            Recorded,
            MapCache,
            ScorecardClock,
            ScorecardStore,
            ScorecardEncoder,
        >,
    ) -> WeatherEdgeDispatchOutcome
    where
        Assets: PublicStaticAssetRead,
        Renderer: PublicStaticTemplateRenderer,
        AdminLoginRenderer: AdminLoginPageRenderer,
        Api: PublicApiProxyClient,
        WidgetClient: WidgetForecastClient,
        WidgetClock: WidgetForecastReceivedAtSource,
        WidgetProjector: WidgetForecastSnapshotProjector<WidgetClient::Forecast>,
        Recorded: ForecastRecordedProductsRead,
        MapCache: XweatherMapTileCache,
        ScorecardClock: ForecastAdjustmentScorecardClock,
        ScorecardStore: ForecastAdjustmentScorecardStore,
        ScorecardEncoder: ForecastAdjustmentScorecardEncoder,
    {
        let mut admin_session = WeatherAdminSessionApplication::new(
            &mut self.sessions,
            &mut self.auth,
            &mut self.clock,
            &mut self.material,
        );
        dispatch_weather_edge_request(
            request,
            WeatherEdgeDispatchPorts {
                admin_login_renderer: routes.admin_login_renderer,
                admin_session: &mut admin_session,
                api: routes.api,
                assets: routes.assets,
                layout: &mut self.layout,
                layout_update_instants: &mut self.layout_update_instants,
                map_cache: routes.map_cache,
                property_sensor_center: self.property_sensor_center,
                recorded: routes.recorded,
                renderer: routes.renderer,
                scorecard_clock: routes.scorecard_clock,
                scorecard_encoder: routes.scorecard_encoder,
                scorecard_store: routes.scorecard_store,
                settings: &mut self.settings,
                widget_client: routes.widget_client,
                widget_clock: routes.widget_clock,
                widget_projector: routes.widget_projector,
            },
        )
    }
}

/// Already-initialized non-admin route owners required by the complete edge dispatcher.
pub struct NativeEdgeRouteOwners<
    'a,
    Assets,
    Renderer,
    AdminLoginRenderer,
    Api,
    WidgetClient,
    WidgetClock,
    WidgetProjector,
    Recorded,
    MapCache,
    ScorecardClock,
    ScorecardStore,
    ScorecardEncoder,
> {
    pub admin_login_renderer: &'a AdminLoginRenderer,
    pub api: &'a Api,
    pub assets: &'a Assets,
    pub map_cache: &'a MapCache,
    pub recorded: &'a Recorded,
    pub renderer: &'a Renderer,
    pub scorecard_clock: &'a ScorecardClock,
    pub scorecard_encoder: &'a ScorecardEncoder,
    pub scorecard_store: &'a ScorecardStore,
    pub widget_client: &'a WidgetClient,
    pub widget_clock: &'a WidgetClock,
    pub widget_projector: &'a WidgetProjector,
}

/// Derive the settings sibling path exactly as the predecessor administrator store.
pub fn native_edge_adjustment_settings_path(layout_path: &Path) -> PathBuf {
    let parent = layout_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    parent.join(ADJUSTMENT_SETTINGS_FILE_NAME)
}

/// Validate the original property placement center range without accepting NaN.
fn valid_property_sensor_center(latitude: f64, longitude: f64) -> bool {
    (-85.0..=85.0).contains(&latitude) && (-180.0..=180.0).contains(&longitude)
}

#[cfg(test)]
#[path = "native_edge_admin_owner_test.rs"]
mod tests;
