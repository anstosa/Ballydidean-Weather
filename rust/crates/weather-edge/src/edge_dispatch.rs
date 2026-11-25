use crate::public_serving_application::{public_text_response, serve_static_asset};
use crate::{
    AdminLoginPageApplicationRequest, AdminLoginPageRenderer, AdminPageApplicationRequest,
    AdmittedAdminMutation, AdmittedAdminOperator, AdmittedAdminRouteInput,
    AdmittedRemoteAgentsPreviewTarget, AdmittedViewerContext, DecodedForecastAdjustmentSettings,
    DecodedPropertySensorLayoutUpdate, DelegatedServingBoundary,
    ForecastAdjustmentScorecardAuthority, ForecastAdjustmentScorecardClock,
    ForecastAdjustmentScorecardEncoder, ForecastAdjustmentScorecardMethod,
    ForecastAdjustmentScorecardRequest, ForecastAdjustmentScorecardResponse,
    ForecastAdjustmentScorecardStore, ForecastAdjustmentSettingsOutcome,
    ForecastAdjustmentSettingsRequest, ForecastAdjustmentSettingsStore,
    ForecastRecordedProductMethod, ForecastRecordedProductRequest, ForecastRecordedProductResponse,
    ForecastRecordedProductRoute, ForecastRecordedProductsRead, PropertySensorLayoutOutcome,
    PropertySensorLayoutRequest, PropertySensorLayoutStore,
    PropertySensorLayoutUpdateInstantSource, PropertySensorMapCenter, PublicApiProxyClient,
    PublicServingApplicationOutcome, PublicServingApplicationRequest,
    PublicServingApplicationResponse, PublicServingPlan, PublicStaticAssetRead,
    PublicStaticTemplateRenderer, RemoteAgentsPreviewResponse, WeatherAdminRouteAllow,
    WeatherAdminRouteOutcome, WeatherAdminSessionApplicationPort, WeatherViewerContextOutcome,
    WidgetForecastApplicationMethod, WidgetForecastApplicationRequest,
    WidgetForecastApplicationResponse, WidgetForecastClient, WidgetForecastReceivedAtSource,
    WidgetForecastSnapshotProjector, XweatherMapProxyMethod, XweatherMapProxyRequest,
    XweatherMapProxyResponse, XweatherMapTileCache, handle_admin_bootstrap, handle_admin_login,
    handle_admin_login_page_application, handle_admin_logout, handle_admin_page_application,
    handle_forecast_adjustment_scorecard, handle_forecast_adjustment_settings,
    handle_forecast_recorded_product, handle_property_sensor_layout, handle_public_static_and_api,
    handle_remote_agents_preview, handle_widget_forecast_application, handle_xweather_map_proxy,
    plan_public_request,
};

/// one decoded public-listener request plus optional admitted private inputs.
#[derive(Debug)]
pub struct WeatherEdgeDispatchRequest<'a> {
    pub accept: Option<&'a str>,
    pub admin_mutation: Option<AdmittedAdminMutation<'a>>,
    pub admin_operator: Option<&'a AdmittedAdminOperator>,
    pub admin_route_input: Option<AdmittedAdminRouteInput<'a>>,
    pub adjustment_settings_body: Option<DecodedForecastAdjustmentSettings>,
    pub has_query_delimiter: bool,
    pub map_request: Option<XweatherMapProxyRequest>,
    pub method: &'a str,
    pub pathname: &'a str,
    pub property_sensor_mutation: Option<AdmittedPropertySensorMutation<'a>>,
    pub remote_agents_preview: Option<AdmittedRemoteAgentsPreviewTarget>,
    pub release: &'a str,
    pub search: &'a str,
    pub viewer_context: Option<AdmittedViewerContext>,
}

/// one property update after session, URI, bounded body, and origin admission.
#[derive(Debug, Clone, PartialEq)]
pub struct AdmittedPropertySensorMutation<'a> {
    pub input: DecodedPropertySensorLayoutUpdate,
    pub mutation: AdmittedAdminMutation<'a>,
    pub sensor_key: String,
}

/// the already-landed application ports used by one public-listener dispatch.
pub struct WeatherEdgeDispatchPorts<
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
    Settings,
    Layout,
    LayoutUpdateInstants,
    ScorecardClock,
    ScorecardStore,
    ScorecardEncoder,
    AdminSession,
> {
    pub admin_login_renderer: &'a AdminLoginRenderer,
    pub admin_session: &'a mut AdminSession,
    pub api: &'a Api,
    pub assets: &'a Assets,
    pub layout: &'a mut Layout,
    pub layout_update_instants: &'a mut LayoutUpdateInstants,
    pub map_cache: &'a MapCache,
    pub property_sensor_center: PropertySensorMapCenter,
    pub recorded: &'a Recorded,
    pub renderer: &'a Renderer,
    pub scorecard_clock: &'a ScorecardClock,
    pub scorecard_encoder: &'a ScorecardEncoder,
    pub scorecard_store: &'a ScorecardStore,
    pub settings: &'a mut Settings,
    pub widget_client: &'a WidgetClient,
    pub widget_clock: &'a WidgetClock,
    pub widget_projector: &'a WidgetProjector,
}

/// typed access to the complete already-landed public-listener application ports.
pub trait WeatherEdgeDispatchPortSet {
    type Assets: PublicStaticAssetRead;
    type Renderer: PublicStaticTemplateRenderer;
    type AdminLoginRenderer: AdminLoginPageRenderer;
    type Api: PublicApiProxyClient;
    type WidgetClient: WidgetForecastClient;
    type WidgetClock: WidgetForecastReceivedAtSource;
    type WidgetProjector: WidgetForecastSnapshotProjector<
        <Self::WidgetClient as WidgetForecastClient>::Forecast,
    >;
    type Recorded: ForecastRecordedProductsRead;
    type MapCache: XweatherMapTileCache;
    type Settings: ForecastAdjustmentSettingsStore;
    type ScorecardClock: ForecastAdjustmentScorecardClock;
    type ScorecardStore: ForecastAdjustmentScorecardStore;
    type ScorecardEncoder: ForecastAdjustmentScorecardEncoder;
    type AdminSession: WeatherAdminSessionApplicationPort;

    /// mutably borrow the administrator session application.
    fn admin_session(&mut self) -> &mut Self::AdminSession;

    /// borrow the public API client.
    fn api(&self) -> &Self::Api;

    /// borrow the selected static asset reader.
    fn assets(&self) -> &Self::Assets;

    /// borrow the administrator login template renderer.
    fn admin_login_renderer(&self) -> &Self::AdminLoginRenderer;

    /// execute one property layout request over its store and update-time port.
    fn property_sensor_layout(
        &mut self,
        request: PropertySensorLayoutRequest<'_>,
    ) -> PropertySensorLayoutOutcome;

    /// borrow the decoded map cache.
    fn map_cache(&self) -> &Self::MapCache;

    /// borrow the recorded-product store.
    fn recorded(&self) -> &Self::Recorded;

    /// borrow the static template renderer.
    fn renderer(&self) -> &Self::Renderer;

    /// borrow the admitted scorecard clock.
    fn scorecard_clock(&self) -> &Self::ScorecardClock;

    /// borrow the admitted scorecard encoder.
    fn scorecard_encoder(&self) -> &Self::ScorecardEncoder;

    /// borrow the admitted scorecard store.
    fn scorecard_store(&self) -> &Self::ScorecardStore;

    /// mutably borrow the adjustment settings store.
    fn settings(&mut self) -> &mut Self::Settings;

    /// borrow the widget upstream client.
    fn widget_client(&self) -> &Self::WidgetClient;

    /// borrow the widget receipt clock.
    fn widget_clock(&self) -> &Self::WidgetClock;

    /// borrow the widget projection owner.
    fn widget_projector(&self) -> &Self::WidgetProjector;
}

impl<
    Assets,
    Renderer,
    AdminLoginRenderer,
    Api,
    WidgetClient,
    WidgetClock,
    WidgetProjector,
    Recorded,
    MapCache,
    Settings,
    Layout,
    LayoutUpdateInstants,
    ScorecardClock,
    ScorecardStore,
    ScorecardEncoder,
    AdminSession,
> WeatherEdgeDispatchPortSet
    for WeatherEdgeDispatchPorts<
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
        Settings,
        Layout,
        LayoutUpdateInstants,
        ScorecardClock,
        ScorecardStore,
        ScorecardEncoder,
        AdminSession,
    >
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
    Settings: ForecastAdjustmentSettingsStore,
    Layout: PropertySensorLayoutStore,
    LayoutUpdateInstants: PropertySensorLayoutUpdateInstantSource,
    ScorecardClock: ForecastAdjustmentScorecardClock,
    ScorecardStore: ForecastAdjustmentScorecardStore,
    ScorecardEncoder: ForecastAdjustmentScorecardEncoder,
    AdminSession: WeatherAdminSessionApplicationPort,
{
    type Assets = Assets;
    type Renderer = Renderer;
    type AdminLoginRenderer = AdminLoginRenderer;
    type Api = Api;
    type WidgetClient = WidgetClient;
    type WidgetClock = WidgetClock;
    type WidgetProjector = WidgetProjector;
    type Recorded = Recorded;
    type MapCache = MapCache;
    type Settings = Settings;
    type ScorecardClock = ScorecardClock;
    type ScorecardStore = ScorecardStore;
    type ScorecardEncoder = ScorecardEncoder;
    type AdminSession = AdminSession;

    /// mutably borrow the administrator session application.
    fn admin_session(&mut self) -> &mut Self::AdminSession {
        self.admin_session
    }

    /// borrow the public API client.
    fn api(&self) -> &Self::Api {
        self.api
    }

    /// borrow the selected static asset reader.
    fn assets(&self) -> &Self::Assets {
        self.assets
    }

    /// borrow the administrator login template renderer.
    fn admin_login_renderer(&self) -> &Self::AdminLoginRenderer {
        self.admin_login_renderer
    }

    /// execute one property layout request over its store and update-time port.
    fn property_sensor_layout(
        &mut self,
        request: PropertySensorLayoutRequest<'_>,
    ) -> PropertySensorLayoutOutcome {
        handle_property_sensor_layout(
            self.property_sensor_center,
            request,
            self.layout,
            self.layout_update_instants,
        )
    }

    /// borrow the decoded map cache.
    fn map_cache(&self) -> &Self::MapCache {
        self.map_cache
    }

    /// borrow the recorded-product store.
    fn recorded(&self) -> &Self::Recorded {
        self.recorded
    }

    /// borrow the static template renderer.
    fn renderer(&self) -> &Self::Renderer {
        self.renderer
    }

    /// borrow the admitted scorecard clock.
    fn scorecard_clock(&self) -> &Self::ScorecardClock {
        self.scorecard_clock
    }

    /// borrow the admitted scorecard encoder.
    fn scorecard_encoder(&self) -> &Self::ScorecardEncoder {
        self.scorecard_encoder
    }

    /// borrow the admitted scorecard store.
    fn scorecard_store(&self) -> &Self::ScorecardStore {
        self.scorecard_store
    }

    /// mutably borrow the adjustment settings store.
    fn settings(&mut self) -> &mut Self::Settings {
        self.settings
    }

    /// borrow the widget upstream client.
    fn widget_client(&self) -> &Self::WidgetClient {
        self.widget_client
    }

    /// borrow the widget receipt clock.
    fn widget_clock(&self) -> &Self::WidgetClock {
        self.widget_clock
    }

    /// borrow the widget projection owner.
    fn widget_projector(&self) -> &Self::WidgetProjector {
        self.widget_projector
    }
}

/// application boundaries that still require their dedicated transport or admission owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeatherEdgeDispatchBoundary {
    ViewerContextAdmission,
    AdminLoginAdmission,
    AdminLogoutAdmission,
    AdminBootstrapAdmission,
    AdminSessionAdmission,
    AdminAdjustmentSettingsMutationAdmission,
    AdminPropertySensorMutationAdmission,
    AdminPageSessionAdmission,
    XweatherMapRequestAdmission,
    RemoteAgentsPreviewAdmission,
    InconsistentPublicPlan,
}

/// one typed result from the real public-listener application dispatcher.
#[derive(Debug, Clone, PartialEq)]
pub enum WeatherEdgeDispatchOutcome {
    Public(PublicServingApplicationResponse),
    Widget(WidgetForecastApplicationResponse),
    Recorded(ForecastRecordedProductResponse),
    Map(XweatherMapProxyResponse),
    AdjustmentSettings(ForecastAdjustmentSettingsOutcome),
    PropertySensorLayout(PropertySensorLayoutOutcome),
    AdjustmentScorecard(ForecastAdjustmentScorecardResponse),
    Admin(WeatherAdminRouteOutcome),
    ViewerContext(WeatherViewerContextOutcome),
    RemoteAgentsPreview(RemoteAgentsPreviewResponse),
    Boundary(WeatherEdgeDispatchBoundary),
}

/// dispatch one parsed public-listener request through the existing typed application owners.
pub fn dispatch_weather_edge_request<Ports>(
    request: WeatherEdgeDispatchRequest<'_>,
    mut ports: Ports,
) -> WeatherEdgeDispatchOutcome
where
    Ports: WeatherEdgeDispatchPortSet,
{
    // widget routes reject even a bare question mark; recorded products use parsed search
    let query_present = !request.search.is_empty()
        || (request.has_query_delimiter && crate::is_widget_forecast_path(request.pathname));
    let plan = plan_public_request(
        request.release,
        request.method,
        request.pathname,
        query_present,
    );

    // preserve the Node listener's specialized-before-proxy route priority
    match plan {
        PublicServingPlan::Widget(_) => {
            WeatherEdgeDispatchOutcome::Widget(handle_widget_forecast_application(
                WidgetForecastApplicationRequest {
                    has_query: request.has_query_delimiter || !request.search.is_empty(),
                    method: widget_method(request.method),
                    pathname: request.pathname,
                },
                ports.widget_client(),
                ports.widget_clock(),
                ports.widget_projector(),
            ))
        }
        PublicServingPlan::RecordedForecast(plan) => {
            WeatherEdgeDispatchOutcome::Recorded(handle_forecast_recorded_product(
                ForecastRecordedProductRequest {
                    has_query: !request.search.is_empty(),
                    method: recorded_method(request.method),
                    route: if plan.solar_cloud_evidence {
                        ForecastRecordedProductRoute::SolarCloudEvidence
                    } else {
                        ForecastRecordedProductRoute::ForecastObservations
                    },
                },
                ports.recorded(),
            ))
        }
        PublicServingPlan::StaticAsset(plan) => {
            // keep the signed-in shell on every html route without varying bundle files
            WeatherEdgeDispatchOutcome::Public(serve_static_asset(
                request.pathname,
                plan,
                request.admin_operator.is_some(),
                ports.assets(),
                ports.renderer(),
            ))
        }
        PublicServingPlan::Reject(rejection) => {
            // preserve the selected rejection without reparsing the normalized search
            WeatherEdgeDispatchOutcome::Public(public_text_response(
                rejection.status_code(),
                rejection.body().as_bytes(),
                rejection.allow(),
            ))
        }
        PublicServingPlan::ApiProxy(_) => {
            // reuse the existing response owner for the ordinary proxy
            match handle_public_static_and_api(
                PublicServingApplicationRequest {
                    accept: request.accept,
                    method: request.method,
                    pathname: request.pathname,
                    release: request.release,
                    search: request.search,
                },
                ports.assets(),
                ports.renderer(),
                ports.api(),
            ) {
                PublicServingApplicationOutcome::Handled(response) => {
                    WeatherEdgeDispatchOutcome::Public(response)
                }
                PublicServingApplicationOutcome::Boundary(_) => {
                    WeatherEdgeDispatchOutcome::Boundary(
                        WeatherEdgeDispatchBoundary::InconsistentPublicPlan,
                    )
                }
            }
        }
        PublicServingPlan::Delegated(boundary) => {
            // connect only boundaries with complete typed application ownership
            match boundary {
                DelegatedServingBoundary::PropertySensorLayout => {
                    let property_request = match request.method {
                        "GET" => PropertySensorLayoutRequest::PublicRead { omit_body: false },
                        "HEAD" => PropertySensorLayoutRequest::PublicRead { omit_body: true },
                        _ => PropertySensorLayoutRequest::PublicUnsupported,
                    };
                    WeatherEdgeDispatchOutcome::PropertySensorLayout(
                        ports.property_sensor_layout(property_request),
                    )
                }
                DelegatedServingBoundary::PublicAdjustmentSettings => {
                    let settings_request = match request.method {
                        "GET" => ForecastAdjustmentSettingsRequest::PublicRead { omit_body: false },
                        "HEAD" => ForecastAdjustmentSettingsRequest::PublicRead { omit_body: true },
                        _ => ForecastAdjustmentSettingsRequest::PublicUnsupported,
                    };
                    WeatherEdgeDispatchOutcome::AdjustmentSettings(
                        handle_forecast_adjustment_settings(settings_request, ports.settings()),
                    )
                }
                DelegatedServingBoundary::AdminAdjustmentSettings => {
                    dispatch_admin_adjustment_settings(request, ports.settings())
                }
                DelegatedServingBoundary::AdminAdjustmentScorecard => {
                    let Some(operator) = request.admin_operator else {
                        return WeatherEdgeDispatchOutcome::Boundary(
                            WeatherEdgeDispatchBoundary::AdminSessionAdmission,
                        );
                    };
                    WeatherEdgeDispatchOutcome::AdjustmentScorecard(
                        handle_forecast_adjustment_scorecard(
                            ForecastAdjustmentScorecardRequest {
                                authority: ForecastAdjustmentScorecardAuthority::Admitted(operator),
                                has_query_delimiter: request.has_query_delimiter,
                                method: scorecard_method(request.method),
                            },
                            ports.scorecard_clock(),
                            ports.scorecard_store(),
                            ports.scorecard_encoder(),
                        ),
                    )
                }
                DelegatedServingBoundary::XweatherMap => {
                    let Some(mut map_request) = request.map_request else {
                        return WeatherEdgeDispatchOutcome::Boundary(
                            WeatherEdgeDispatchBoundary::XweatherMapRequestAdmission,
                        );
                    };
                    map_request.method = map_method(request.method);
                    WeatherEdgeDispatchOutcome::Map(handle_xweather_map_proxy(
                        map_request,
                        ports.map_cache(),
                    ))
                }
                DelegatedServingBoundary::ViewerContext => dispatch_viewer_context(request),
                DelegatedServingBoundary::AdminLogin => {
                    dispatch_admin_login(request, ports.admin_session())
                }
                DelegatedServingBoundary::AdminLogout => {
                    dispatch_admin_logout(request, ports.admin_session())
                }
                DelegatedServingBoundary::AdminBootstrap => {
                    dispatch_admin_bootstrap(request, ports.admin_session())
                }
                DelegatedServingBoundary::AdminPropertySensorUpdate => {
                    let property_request = if request.method == "PUT" {
                        // require admitted URI, body, session, and mutation evidence
                        let Some(admitted) = request.property_sensor_mutation else {
                            return WeatherEdgeDispatchOutcome::Boundary(
                                WeatherEdgeDispatchBoundary::AdminPropertySensorMutationAdmission,
                            );
                        };
                        PropertySensorLayoutRequest::AdminReplace {
                            mutation: admitted.mutation,
                            sensor_key: admitted.sensor_key,
                            input: admitted.input,
                        }
                    } else {
                        // preserve authentication before the private method response
                        let Some(operator) = request.admin_operator else {
                            return WeatherEdgeDispatchOutcome::Boundary(
                                WeatherEdgeDispatchBoundary::AdminSessionAdmission,
                            );
                        };
                        PropertySensorLayoutRequest::AdminUnsupported { operator }
                    };
                    WeatherEdgeDispatchOutcome::PropertySensorLayout(
                        ports.property_sensor_layout(property_request),
                    )
                }
                DelegatedServingBoundary::AdminPage => dispatch_admin_page(request, &mut ports),
                DelegatedServingBoundary::RemoteAgentsPreview => {
                    let Some(target) = request.remote_agents_preview else {
                        return WeatherEdgeDispatchOutcome::Boundary(
                            WeatherEdgeDispatchBoundary::RemoteAgentsPreviewAdmission,
                        );
                    };
                    WeatherEdgeDispatchOutcome::RemoteAgentsPreview(handle_remote_agents_preview(
                        target,
                    ))
                }
            }
        }
    }
}

/// dispatch the inline viewer projection after method and matcher admission.
fn dispatch_viewer_context(request: WeatherEdgeDispatchRequest<'_>) -> WeatherEdgeDispatchOutcome {
    let omit_body = match request.method {
        "GET" => false,
        "HEAD" => true,
        _ => {
            return WeatherEdgeDispatchOutcome::ViewerContext(
                WeatherViewerContextOutcome::MethodNotAllowed,
            );
        }
    };
    // require the existing matcher result instead of inventing network eligibility
    let Some(viewer) = request.viewer_context else {
        return WeatherEdgeDispatchOutcome::Boundary(
            WeatherEdgeDispatchBoundary::ViewerContextAdmission,
        );
    };
    WeatherEdgeDispatchOutcome::ViewerContext(WeatherViewerContextOutcome::Success {
        home_network: viewer.home_network,
        omit_body,
    })
}

/// dispatch the login form only after credential and cookie-context admission.
fn dispatch_admin_login<Port: WeatherAdminSessionApplicationPort>(
    request: WeatherEdgeDispatchRequest<'_>,
    port: &mut Port,
) -> WeatherEdgeDispatchOutcome {
    // preserve method rejection before form or credential work
    if request.method != "POST" {
        return WeatherEdgeDispatchOutcome::Admin(WeatherAdminRouteOutcome::MethodNotAllowed {
            allow: WeatherAdminRouteAllow::Post,
        });
    }
    let Some(AdmittedAdminRouteInput::Login {
        cookie_mode,
        credential,
    }) = request.admin_route_input
    else {
        return WeatherEdgeDispatchOutcome::Boundary(
            WeatherEdgeDispatchBoundary::AdminLoginAdmission,
        );
    };
    WeatherEdgeDispatchOutcome::Admin(handle_admin_login(&credential, cookie_mode, port))
}

/// dispatch logout only after cookie and transport-context admission.
fn dispatch_admin_logout<Port: WeatherAdminSessionApplicationPort>(
    request: WeatherEdgeDispatchRequest<'_>,
    port: &mut Port,
) -> WeatherEdgeDispatchOutcome {
    // preserve method rejection before cookie parsing or revocation
    if request.method != "POST" {
        return WeatherEdgeDispatchOutcome::Admin(WeatherAdminRouteOutcome::MethodNotAllowed {
            allow: WeatherAdminRouteAllow::Post,
        });
    }
    let Some(AdmittedAdminRouteInput::Logout { cookie_mode, token }) = request.admin_route_input
    else {
        return WeatherEdgeDispatchOutcome::Boundary(
            WeatherEdgeDispatchBoundary::AdminLogoutAdmission,
        );
    };
    WeatherEdgeDispatchOutcome::Admin(handle_admin_logout(token, cookie_mode, port))
}

/// dispatch bootstrap only after its verified credential state is admitted.
fn dispatch_admin_bootstrap<Port: WeatherAdminSessionApplicationPort>(
    request: WeatherEdgeDispatchRequest<'_>,
    port: &mut Port,
) -> WeatherEdgeDispatchOutcome {
    // preserve method rejection before bounded body and credential work
    if request.method != "POST" {
        return WeatherEdgeDispatchOutcome::Admin(WeatherAdminRouteOutcome::MethodNotAllowed {
            allow: WeatherAdminRouteAllow::Post,
        });
    }
    let Some(AdmittedAdminRouteInput::Bootstrap(state)) = request.admin_route_input else {
        return WeatherEdgeDispatchOutcome::Boundary(
            WeatherEdgeDispatchBoundary::AdminBootstrapAdmission,
        );
    };
    WeatherEdgeDispatchOutcome::Admin(handle_admin_bootstrap(state, port))
}

/// dispatch the authenticated administrator shell through the existing template owner.
fn dispatch_admin_page<Ports: WeatherEdgeDispatchPortSet>(
    request: WeatherEdgeDispatchRequest<'_>,
    ports: &mut Ports,
) -> WeatherEdgeDispatchOutcome {
    let Some(AdmittedAdminRouteInput::Page { login, token }) = request.admin_route_input else {
        return WeatherEdgeDispatchOutcome::Boundary(
            WeatherEdgeDispatchBoundary::AdminPageSessionAdmission,
        );
    };
    let authenticated = match token {
        // preserve the normal absent-cookie login representation without inventing a token
        None => Ok(false),
        Some(token) => ports.admin_session().authenticate(token),
    };

    // preserve the login representation and store failure without opening the admin shell
    match authenticated {
        Ok(true) => WeatherEdgeDispatchOutcome::Public(handle_admin_page_application(
            AdminPageApplicationRequest {
                omit_body: request.method == "HEAD",
                pathname: request.pathname,
            },
            ports.assets(),
            ports.renderer(),
        )),
        Ok(false) => WeatherEdgeDispatchOutcome::Public(handle_admin_login_page_application(
            AdminLoginPageApplicationRequest {
                invalid_credentials: login.invalid_credentials,
                omit_body: request.method == "HEAD",
            },
            ports.assets(),
            ports.admin_login_renderer(),
        )),
        Err(_) => WeatherEdgeDispatchOutcome::Admin(WeatherAdminRouteOutcome::StoreFailure),
    }
}

/// dispatch one settings route only after its external session and mutation admissions.
fn dispatch_admin_adjustment_settings<Store: ForecastAdjustmentSettingsStore>(
    request: WeatherEdgeDispatchRequest<'_>,
    settings: &mut Store,
) -> WeatherEdgeDispatchOutcome {
    let settings_request = match request.method {
        "GET" | "HEAD" => {
            // require the session owner before every admitted private read
            let Some(operator) = request.admin_operator else {
                return WeatherEdgeDispatchOutcome::Boundary(
                    WeatherEdgeDispatchBoundary::AdminSessionAdmission,
                );
            };
            ForecastAdjustmentSettingsRequest::AdminRead {
                operator,
                omit_body: request.method == "HEAD",
            }
        }
        "PUT" => {
            // require both same-origin mutation proof and decoded bounded body evidence
            let (Some(mutation), Some(input)) =
                (request.admin_mutation, request.adjustment_settings_body)
            else {
                return WeatherEdgeDispatchOutcome::Boundary(
                    WeatherEdgeDispatchBoundary::AdminAdjustmentSettingsMutationAdmission,
                );
            };
            ForecastAdjustmentSettingsRequest::AdminReplace { mutation, input }
        }
        _ => {
            // preserve authentication before private method rejection
            let Some(operator) = request.admin_operator else {
                return WeatherEdgeDispatchOutcome::Boundary(
                    WeatherEdgeDispatchBoundary::AdminSessionAdmission,
                );
            };
            ForecastAdjustmentSettingsRequest::AdminUnsupported { operator }
        }
    };
    WeatherEdgeDispatchOutcome::AdjustmentSettings(handle_forecast_adjustment_settings(
        settings_request,
        settings,
    ))
}

/// map the listener method into the widget application's closed method set.
fn widget_method(method: &str) -> WidgetForecastApplicationMethod {
    // preserve case-sensitive Node method identities
    match method {
        "GET" => WidgetForecastApplicationMethod::Get,
        "HEAD" => WidgetForecastApplicationMethod::Head,
        _ => WidgetForecastApplicationMethod::Unsupported,
    }
}

/// map the listener method into the recorded-product application's method set.
fn recorded_method(method: &str) -> ForecastRecordedProductMethod {
    // preserve case-sensitive Node method identities
    match method {
        "GET" => ForecastRecordedProductMethod::Get,
        "HEAD" => ForecastRecordedProductMethod::Head,
        _ => ForecastRecordedProductMethod::Unsupported,
    }
}

/// map the listener method into the Xweather application's method set.
fn map_method(method: &str) -> XweatherMapProxyMethod {
    // preserve case-sensitive Node method identities
    match method {
        "GET" => XweatherMapProxyMethod::Get,
        "HEAD" => XweatherMapProxyMethod::Head,
        _ => XweatherMapProxyMethod::Unsupported,
    }
}

/// map the listener method into the private scorecard application's method set.
fn scorecard_method(method: &str) -> ForecastAdjustmentScorecardMethod {
    // preserve case-sensitive Node method identities
    match method {
        "GET" => ForecastAdjustmentScorecardMethod::Get,
        "HEAD" => ForecastAdjustmentScorecardMethod::Head,
        _ => ForecastAdjustmentScorecardMethod::Unsupported,
    }
}
