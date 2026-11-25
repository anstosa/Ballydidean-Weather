//! Shared native public route owners with request-scoped administrator state access.

use std::sync::{Arc, Mutex};
use weather_edge::{
    AdmittedAdminCredential, AdmittedAdminPersistentAuthState, AdmittedAdminSessionToken,
    FileForecastAdjustmentScorecardStore, NativePublicAssetReader, NativePublicTemplates,
    PropertySensorLayoutOutcome, PropertySensorLayoutRequest, PublicAdjustmentProjector,
    SystemPublicTemplateClock, WeatherAdminBootstrapStatus, WeatherAdminSessionApplicationPort,
    WeatherAdminSessionResponse, WeatherAdminStoreError, WeatherEdgeDispatchPortSet,
};

use crate::{
    NativeEdgeAdminOwner, NativeEdgeConfiguration, NativeEdgePublicAssets,
    NativeEdgePublicUpstreams, NativeEdgeRecordedOwner, NativeEdgeRecordedOwnerCreateError,
    NativeEdgeScorecardAdapter, NativeEdgeTileOwner, NativeEdgeTileOwnerCreateError,
    NativeEdgeViewerContextOwner, NativeForecastRecordedProjectionOwner, NativeOutboundHttp,
    NativeOutboundHttpError, NativeProcessClock, NativePublicApiProxy,
    NativeSharedAdjustmentSettings, NativeWidgetForecastProjection, NativeWidgetForecastSource,
    NativeWidgetReceivedAt, NativeWirePrimitives, initialize_native_edge_public_upstreams,
    initialize_native_edge_tile_owner,
};

type Api = NativePublicApiProxy<
    PublicAdjustmentProjector<NativeSharedAdjustmentSettings, NativeWirePrimitives>,
>;
type Templates = NativePublicTemplates<NativeWirePrimitives, SystemPublicTemplateClock>;
type Recorded = NativeEdgeRecordedOwner<NativeForecastRecordedProjectionOwner>;
type Scorecard = FileForecastAdjustmentScorecardStore<NativeEdgeScorecardAdapter>;

/// Actual edge public route graph over one shared client and persisted administrator owner.
///
/// Evidence preparation and terminal settlement remain the existing outer wire owner's responsibility.
/// The administrator mutex is never held across ordinary static, proxy, recorded, or map requests.
pub struct NativeEdgePublicRouteGraph {
    admin: Mutex<NativeEdgeAdminOwner>,
    settings: NativeSharedAdjustmentSettings,
    public: NativeEdgePublicAssets,
    upstreams: NativeEdgePublicUpstreams,
    recorded: Recorded,
    tile: NativeEdgeTileOwner,
    viewer: NativeEdgeViewerContextOwner,
    map_site: Option<weather_edge::AdmittedXweatherMapSite>,
    scorecard: Scorecard,
    scorecard_clock: NativeProcessClock,
    scorecard_encoder: NativeEdgeScorecardAdapter,
    widget_projector: NativeWidgetForecastProjection,
}

/// Distinguish real outbound, recorded, or map owner initialization failures.
#[derive(Debug)]
pub enum NativeEdgePublicRouteGraphError {
    Upstream(NativeOutboundHttpError),
    Recorded(NativeEdgeRecordedOwnerCreateError),
    Tile(NativeEdgeTileOwnerCreateError),
}

impl NativeEdgePublicRouteGraph {
    /// Retain the already initialized startup assets/admin and bind the real public route owners.
    pub fn initialize(
        startup: &mut NativeEdgeConfiguration,
        admin: NativeEdgeAdminOwner,
        public: NativeEdgePublicAssets,
        site: Option<weather_edge::ForecastMapSite>,
        client: Arc<NativeOutboundHttp>,
    ) -> Result<Self, NativeEdgePublicRouteGraphError> {
        let settings = admin.shared_adjustment_settings();
        let upstreams = initialize_native_edge_public_upstreams(
            &startup.configuration,
            &admin,
            Arc::clone(&client),
        )
        .map_err(NativeEdgePublicRouteGraphError::Upstream)?;
        let recorded = NativeEdgeRecordedOwner::new(
            startup.configuration.api_origin.clone(),
            Arc::clone(&client),
            NativeForecastRecordedProjectionOwner,
        )
        .map_err(NativeEdgePublicRouteGraphError::Recorded)?;
        let viewer = NativeEdgeViewerContextOwner::new(Arc::clone(&client));
        let tile = initialize_native_edge_tile_owner(startup, client)
            .map_err(NativeEdgePublicRouteGraphError::Tile)?;
        let scorecard = FileForecastAdjustmentScorecardStore::new(
            startup.configuration.adjustment_evidence_root.clone(),
            NativeEdgeScorecardAdapter,
        );
        Ok(Self {
            admin: Mutex::new(admin),
            settings,
            public,
            upstreams,
            recorded,
            tile,
            viewer,
            map_site: site.and_then(crate::native_edge_tile_site),
            scorecard,
            scorecard_clock: NativeProcessClock,
            scorecard_encoder: NativeEdgeScorecardAdapter,
            widget_projector: NativeWidgetForecastProjection,
        })
    }

    /// Borrow the same actual administrator owner for request-scoped admission.
    pub const fn admin(&self) -> &Mutex<NativeEdgeAdminOwner> {
        &self.admin
    }

    /// Borrow the retained map owner for the existing path/time/provider admission.
    pub const fn tile(&self) -> &NativeEdgeTileOwner {
        &self.tile
    }

    /// Borrow the same process viewer-context owner for route-specific metadata admission.
    pub const fn viewer(&self) -> &NativeEdgeViewerContextOwner {
        &self.viewer
    }

    /// Admit the native viewer, map and preview route inputs after request URL parsing.
    pub fn admit_non_admin_routes(
        &self,
        request: &crate::NativeEdgeHttpRequest,
        admissions: &mut crate::NativeEdgeRequestAdmissions<'_>,
    ) -> Result<(), NativeEdgeOrdinaryAdmissionError> {
        let pathname = &request.target().pathname;
        let method = request.parts().method.as_str();
        // avoid matcher or URL work on routes rejected by the existing method policy
        if pathname == "/api/v1/viewer-context" && matches!(method, "GET" | "HEAD") {
            admissions.viewer_context = Some(self.viewer.viewer_context(request));
        } else if pathname.starts_with("/maps/xweather/") {
            let method = match method {
                "GET" => weather_edge::XweatherMapProxyMethod::Get,
                "HEAD" => weather_edge::XweatherMapProxyMethod::Head,
                _ => weather_edge::XweatherMapProxyMethod::Unsupported,
            };
            let site = self
                .map_site
                .ok_or(NativeEdgeOrdinaryAdmissionError::MapSiteUnavailable(method))?;
            admissions.map_request = Some(self.tile.request(method, pathname, site));
        } else if pathname == "/__rac/browser-device" && matches!(method, "GET" | "HEAD") {
            admissions.remote_agents_preview = Some(
                crate::admit_native_edge_preview(request.target())
                    .map_err(|_| NativeEdgeOrdinaryAdmissionError::InvalidPreview)?,
            );
        }
        Ok(())
    }

    /// Build short-lived route ports without taking the administrator lock.
    pub fn ports(&self) -> NativeEdgePublicRoutePorts<'_> {
        NativeEdgePublicRoutePorts {
            graph: self,
            session: NativeSharedAdminSession(&self.admin),
            settings: self.settings.clone(),
        }
    }
}

/// A missing source-site map admission or invalid browser preview retains its ordinary response category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeEdgeOrdinaryAdmissionError {
    MapSiteUnavailable(weather_edge::XweatherMapProxyMethod),
    InvalidPreview,
}

/// Request-scoped session port that locks only for actual admin operations.
pub struct NativeSharedAdminSession<'a>(&'a Mutex<NativeEdgeAdminOwner>);

impl WeatherAdminSessionApplicationPort for NativeSharedAdminSession<'_> {
    /// Persist the genuine admitted bootstrap state through the shared process owner.
    fn bootstrap(
        &mut self,
        state: AdmittedAdminPersistentAuthState,
    ) -> Result<WeatherAdminBootstrapStatus, WeatherAdminStoreError> {
        self.0
            .lock()
            .map_err(|_| WeatherAdminStoreError::Persistence)?
            .session_application()
            .bootstrap(state)
    }

    /// Exchange one verified credential for the existing process session.
    fn login(
        &mut self,
        credential: &AdmittedAdminCredential,
    ) -> Result<Option<WeatherAdminSessionResponse>, WeatherAdminStoreError> {
        self.0
            .lock()
            .map_err(|_| WeatherAdminStoreError::Persistence)?
            .session_application()
            .login(credential)
    }

    /// Revoke the admitted browser session through the same process registry.
    fn logout(
        &mut self,
        token: Option<&AdmittedAdminSessionToken>,
    ) -> Result<(), WeatherAdminStoreError> {
        self.0
            .lock()
            .map_err(|_| WeatherAdminStoreError::Persistence)?
            .session_application()
            .logout(token)
    }

    /// Authenticate through the retained process registry without a separate session store.
    fn authenticate(
        &mut self,
        token: &AdmittedAdminSessionToken,
    ) -> Result<bool, WeatherAdminStoreError> {
        self.0
            .lock()
            .map_err(|_| WeatherAdminStoreError::Persistence)?
            .session_application()
            .authenticate(token)
    }
}

/// Concrete request-scoped dispatcher ports, retaining no lock during general HTTP work.
pub struct NativeEdgePublicRoutePorts<'a> {
    graph: &'a NativeEdgePublicRouteGraph,
    session: NativeSharedAdminSession<'a>,
    settings: NativeSharedAdjustmentSettings,
}

impl<'a> WeatherEdgeDispatchPortSet for NativeEdgePublicRoutePorts<'a> {
    type Assets = NativePublicAssetReader;
    type Renderer = Templates;
    type AdminLoginRenderer = Templates;
    type Api = Api;
    type WidgetClient = NativeWidgetForecastSource<NativeSharedAdjustmentSettings>;
    type WidgetClock = NativeWidgetReceivedAt;
    type WidgetProjector = NativeWidgetForecastProjection;
    type Recorded = Recorded;
    type MapCache = NativeEdgeTileOwner;
    type Settings = NativeSharedAdjustmentSettings;
    type ScorecardClock = NativeProcessClock;
    type ScorecardStore = Scorecard;
    type ScorecardEncoder = NativeEdgeScorecardAdapter;
    type AdminSession = NativeSharedAdminSession<'a>;

    /// Borrow the request-scoped session adapter.
    fn admin_session(&mut self) -> &mut Self::AdminSession {
        &mut self.session
    }
    /// Borrow the retained upstream API proxy.
    fn api(&self) -> &Self::Api {
        &self.graph.upstreams.api
    }
    /// Borrow the immutable asset reader.
    fn assets(&self) -> &Self::Assets {
        &self.graph.public.assets
    }
    /// Borrow the shared login template renderer.
    fn admin_login_renderer(&self) -> &Self::AdminLoginRenderer {
        &self.graph.public.templates
    }
    /// Execute only the actual sensor layout route while holding the admin state.
    fn property_sensor_layout(
        &mut self,
        request: PropertySensorLayoutRequest<'_>,
    ) -> PropertySensorLayoutOutcome {
        self.graph
            .admin
            .lock()
            .map_or(PropertySensorLayoutOutcome::StoreFailure, |mut admin| {
                admin.property_sensor_layout(request)
            })
    }
    /// Borrow the retained map cache.
    fn map_cache(&self) -> &Self::MapCache {
        &self.graph.tile
    }
    /// Borrow the retained recorded product cache.
    fn recorded(&self) -> &Self::Recorded {
        &self.graph.recorded
    }
    /// Borrow the immutable public template renderer.
    fn renderer(&self) -> &Self::Renderer {
        &self.graph.public.templates
    }
    /// Borrow the actual scorecard clock.
    fn scorecard_clock(&self) -> &Self::ScorecardClock {
        &self.graph.scorecard_clock
    }
    /// Borrow the actual compact scorecard encoder.
    fn scorecard_encoder(&self) -> &Self::ScorecardEncoder {
        &self.graph.scorecard_encoder
    }
    /// Borrow the existing validated file scorecard store.
    fn scorecard_store(&self) -> &Self::ScorecardStore {
        &self.graph.scorecard
    }
    /// Borrow the independently shared persisted settings owner.
    fn settings(&mut self) -> &mut Self::Settings {
        &mut self.settings
    }
    /// Borrow the actual native widget forecast source.
    fn widget_client(&self) -> &Self::WidgetClient {
        &self.graph.upstreams.widget
    }
    /// Borrow the actual post-fetch receipt clock.
    fn widget_clock(&self) -> &Self::WidgetClock {
        &self.graph.upstreams.widget_clock
    }
    /// Borrow the native versioned widget projector.
    fn widget_projector(&self) -> &Self::WidgetProjector {
        &self.graph.widget_projector
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Require the real route graph to move work off the listener reactor without global serialization.
    #[test]
    fn native_edge_route_graph_can_share_blocking_route_workers() {
        fn assert_shareable<T: Send + Sync>() {}
        assert_shareable::<NativeEdgePublicRouteGraph>();
    }

    /// Exercise the actual shared route graph against the shipped immutable shell.
    #[test]
    fn native_edge_route_graph_serves_actual_static_asset() {
        use std::{
            io,
            path::{Path, PathBuf},
        };
        use weather_edge::{
            EdgeStartupEnvironment, EdgeXweatherCredentialByteReader, NativeEdgeStartupAdmission,
            PublicServingApplicationBody, WeatherEdgeDispatchOutcome, WeatherEdgeDispatchRequest,
            dispatch_weather_edge_request, project_edge_startup_configuration_with_credentials,
        };

        struct MissingCredentials;

        impl EdgeXweatherCredentialByteReader for MissingCredentials {
            type Error = io::Error;

            /// Keep fixture startup independent from real credential files.
            fn read_optional(&mut self, _: &Path) -> Result<Option<Vec<u8>>, Self::Error> {
                Ok(None)
            }
        }

        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .canonicalize()
            .unwrap();
        let (configuration, map_credentials) = project_edge_startup_configuration_with_credentials(
            EdgeStartupEnvironment::new(&root),
            &mut NativeEdgeStartupAdmission::new(crate::NativeEdgeOrigins),
            &mut MissingCredentials,
        )
        .unwrap();
        let mut startup = NativeEdgeConfiguration {
            configuration,
            map_credentials,
        };
        let site = crate::native_edge_map_site(&startup.configuration);
        let fixture_path =
            std::env::temp_dir().join(format!("weather-edge-route-fixture-{}", std::process::id()));
        let admin = NativeEdgeAdminOwner::new(
            fixture_path.join("auth.json"),
            fixture_path.join("settings.json"),
            fixture_path.join("layout.json"),
            weather_edge::PropertySensorMapCenter {
                latitude: 47.95043,
                longitude: -122.42797,
            },
        )
        .unwrap();
        let assets =
            crate::initialize_native_edge_public_assets(&startup.configuration, site).unwrap();
        let graph = NativeEdgePublicRouteGraph::initialize(
            &mut startup,
            admin,
            assets,
            site,
            Arc::new(NativeOutboundHttp::new().unwrap()),
        )
        .unwrap();
        let response = dispatch_weather_edge_request(
            WeatherEdgeDispatchRequest {
                accept: Some("text/html"),
                admin_mutation: None,
                admin_operator: None,
                admin_route_input: None,
                adjustment_settings_body: None,
                has_query_delimiter: false,
                map_request: None,
                method: "GET",
                pathname: "/forecast",
                property_sensor_mutation: None,
                remote_agents_preview: None,
                release: &startup.configuration.release,
                search: "",
                viewer_context: None,
            },
            graph.ports(),
        );
        let WeatherEdgeDispatchOutcome::Public(response) = response else {
            panic!("expected ordinary static route");
        };
        assert_eq!(response.status_code, 200);
        let PublicServingApplicationBody::Bytes(Some(bytes)) = response.body else {
            panic!("expected real shell bytes");
        };
        assert!(
            String::from_utf8(bytes)
                .unwrap()
                .contains("/assets/development/")
        );
    }
}
