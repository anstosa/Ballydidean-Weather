use std::cell::{Cell, RefCell};
use std::convert::Infallible;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::process::Command;
use std::rc::Rc;

use weather_edge::{
    ADMIN_SESSION_TTL_SECONDS, AdminLoginPageRenderRequest, AdminLoginPageRenderer,
    AdmittedAdminCookieMode, AdmittedAdminCredential, AdmittedAdminCredentialDigest,
    AdmittedAdminLoginPage, AdmittedAdminMutation, AdmittedAdminOperator,
    AdmittedAdminPasswordVerifier, AdmittedAdminPersistentAuthState, AdmittedAdminRouteInput,
    AdmittedAdminSessionDigest, AdmittedAdminSessionNow, AdmittedAdminSessionToken,
    AdmittedForecastAdjustmentScorecardJson, AdmittedForecastAdjustmentScorecardNow,
    AdmittedForecastAdjustmentScorecardResponseJson, AdmittedForecastAdjustmentScorecardSnapshot,
    AdmittedForecastObservationsJson, AdmittedForecastRecordedProducts,
    AdmittedPropertySensorMutation, AdmittedPropertySensorUpdateInstant,
    AdmittedPublicApiUpstreamResponse, AdmittedPublicAssetBytes, AdmittedRemoteAgentsPreviewTarget,
    AdmittedSolarCloudEvidenceJson, AdmittedViewerContext, AdmittedWidgetForecastReceivedAt,
    AdmittedWidgetForecastSnapshotJson, AdmittedXweatherMapSite, AdmittedXweatherProvider,
    AdmittedXweatherValidTime, DecodedForecastAdjustmentSettings,
    DecodedPropertySensorLayoutUpdate, DecodedXweatherFrameRoute, DecodedXweatherMapRoute,
    EdgeForecastCaptureAdmission, EdgeForecastTerminalObserver, EdgeWireAdminCookie, EdgeWireBody,
    EdgeWireHeaders, EdgeWireJsonEncoder, EdgeWireWriter, ForecastAdjustmentScorecardCandidates,
    ForecastAdjustmentScorecardClock, ForecastAdjustmentScorecardContractVersion,
    ForecastAdjustmentScorecardEncoder, ForecastAdjustmentScorecardPublicationState,
    ForecastAdjustmentScorecardStore, ForecastAdjustmentSettingsEnvelope,
    ForecastAdjustmentSettingsOutcome, ForecastArchiveAppendOutcome, ForecastCaptureSource,
    ForecastCaptureVersion, ForecastEvidenceOutcome, ForecastEvidenceWindow,
    ForecastRecordedProductBody, ForecastRecordedProductsRead, ForecastResponseEvidenceStore,
    ForecastResponseTerminal, ForecastScheduleArchive, ForecastScheduledEvidenceArchive,
    InMemoryForecastAdjustmentSettingsStore, InMemoryPropertySensorLayoutStore,
    PropertySensorLayoutEntry, PropertySensorLayoutOutcome,
    PropertySensorLayoutUpdateInstantSource, PropertySensorMapCenter, PublicApiProxyClient,
    PublicApiProxyClientRequest, PublicServingApplicationBody, PublicServingHeaderProfile,
    PublicStaticAssetRead, PublicStaticTemplateRenderRequest, PublicStaticTemplateRenderer,
    RemoteAgentsPreviewMode, RevisionArchiveCanonicalClock, StaticAssetRoot,
    WeatherAdminAuthStatePort, WeatherAdminBootstrapStatus, WeatherAdminPortError,
    WeatherAdminRouteOutcome, WeatherAdminSessionApplication, WeatherAdminSessionApplicationPort,
    WeatherAdminSessionClock, WeatherAdminSessionMaterialPort, WeatherAdminSessionResponse,
    WeatherAdminStore, WeatherAdminStoreError, WeatherEdgeDispatchOutcome,
    WeatherEdgeDispatchPortSet, WeatherEdgeDispatchPorts, WeatherEdgeDispatchRequest,
    WeatherViewerContextOutcome, WidgetForecastApplicationBody, WidgetForecastClient,
    WidgetForecastClientRequest, WidgetForecastReceivedAtSource, WidgetForecastSnapshotProjector,
    WidgetForecastVersion, XweatherMapProxyBody, XweatherMapProxyMethod, XweatherMapProxyRequest,
    XweatherMapTileCacheError, XweatherTileCacheKey, XweatherTileClock, XweatherTileLoader,
    XweatherTileMemoryCache, dispatch_weather_edge_request, emit_edge_wire_response,
    handle_edge_wire_request, persist_forecast_response_capture,
};

const RELEASE: &str = "2026.11.23-1";
const MAP_NOW_EPOCH_MS: i64 = 1_787_918_400_000;

#[derive(Debug, Default)]
struct FixtureAssets;

impl PublicStaticAssetRead for FixtureAssets {
    type Error = Infallible;

    /// return one complete fixture asset after route selection.
    fn read_asset(
        &self,
        _root: StaticAssetRoot,
        relative_path: &str,
    ) -> Result<AdmittedPublicAssetBytes, Self::Error> {
        Ok(AdmittedPublicAssetBytes::admitted(format!(
            "asset:{relative_path}"
        )))
    }
}

#[derive(Debug, Default)]
struct FixtureRenderer;

impl PublicStaticTemplateRenderer for FixtureRenderer {
    type Error = Infallible;

    /// retain the admitted fixture bytes without adding a codec.
    fn render_template(
        &self,
        source: &AdmittedPublicAssetBytes,
        request: PublicStaticTemplateRenderRequest<'_>,
    ) -> Result<AdmittedPublicAssetBytes, Self::Error> {
        // bind the private shell to the exact administrator representation
        if request.pathname == "/admin" {
            assert!(request.administrator);
            assert!(!request.analytics_enabled);
        } else if request.pathname == "/settings" && request.administrator {
            assert!(!request.analytics_enabled);
            return Ok(AdmittedPublicAssetBytes::admitted(
                &b"admitted settings"[..],
            ));
        } else {
            assert!(!request.administrator);
        }
        Ok(AdmittedPublicAssetBytes::admitted(source.as_bytes()))
    }
}

#[derive(Debug, Default)]
struct FixtureApi {
    calls: Cell<usize>,
    projected_forecast_bytes: Cell<usize>,
    received_headers_and_search: RefCell<Vec<(String, String)>>,
}

impl PublicApiProxyClient for FixtureApi {
    type Error = Infallible;

    /// return one ordinary upstream response and record proxy selection.
    fn fetch_api(
        &self,
        request: PublicApiProxyClientRequest<'_>,
    ) -> Result<AdmittedPublicApiUpstreamResponse, Self::Error> {
        self.calls.set(self.calls.get() + 1);
        self.received_headers_and_search
            .borrow_mut()
            .push((request.accept.to_owned(), request.search.to_owned()));
        let mut body = format!("{{\"path\":\"{}\"}}", request.pathname).into_bytes();
        // allow a valid projected representation to exceed a disposable socket buffer
        if request.pathname == "/api/v1/sites/ballydidean/forecast" {
            body.resize(body.len().max(self.projected_forecast_bytes.get()), b' ');
        }
        Ok(AdmittedPublicApiUpstreamResponse {
            body,
            content_length: Some(24),
            content_type: Some("application/json; charset=utf-8".to_owned()),
            status_code: 200,
            successful: true,
        })
    }
}

#[derive(Debug, Default)]
struct FixtureWidgetClient {
    calls: Cell<usize>,
}

impl WidgetForecastClient for FixtureWidgetClient {
    type Error = Infallible;
    type Forecast = WidgetForecastVersion;

    /// return the selected version as one typed fixture forecast.
    fn fetch_forecast(
        &self,
        request: WidgetForecastClientRequest,
    ) -> Result<Self::Forecast, Self::Error> {
        self.calls.set(self.calls.get() + 1);
        Ok(request.version)
    }
}

#[derive(Debug, Default)]
struct FixtureWidgetClock;

impl WidgetForecastReceivedAtSource for FixtureWidgetClock {
    type Error = Infallible;

    /// return one admitted canonical projection instant.
    fn approved_received_at(&self) -> Result<AdmittedWidgetForecastReceivedAt, Self::Error> {
        Ok(AdmittedWidgetForecastReceivedAt::admitted(
            "2026-11-23T12:00:00.000Z",
        ))
    }
}

#[derive(Debug, Default)]
struct FixtureWidgetProjector;

impl WidgetForecastSnapshotProjector<WidgetForecastVersion> for FixtureWidgetProjector {
    type Error = Infallible;

    /// return one admitted snapshot bound to the selected version.
    fn project_snapshot(
        &self,
        forecast: &WidgetForecastVersion,
        _received_at: &AdmittedWidgetForecastReceivedAt,
        version: WidgetForecastVersion,
    ) -> Result<AdmittedWidgetForecastSnapshotJson, Self::Error> {
        assert_eq!(*forecast, version);
        Ok(AdmittedWidgetForecastSnapshotJson::admitted(
            version,
            b"{\"widget\":true}",
        ))
    }
}

#[derive(Debug)]
struct FixtureRecorded {
    calls: Cell<usize>,
    forecast_observations: AdmittedForecastObservationsJson,
    solar_cloud_evidence: AdmittedSolarCloudEvidenceJson,
}

impl Default for FixtureRecorded {
    /// retain one admitted fixture allocation for each recorded product.
    fn default() -> Self {
        Self {
            calls: Cell::new(0),
            forecast_observations: AdmittedForecastObservationsJson::admitted(b"{\"data\":[]}"),
            solar_cloud_evidence: AdmittedSolarCloudEvidenceJson::admitted(b"{\"evidence\":{}}"),
        }
    }
}

impl ForecastRecordedProductsRead for FixtureRecorded {
    type Error = Infallible;

    /// return one complete pair of admitted recorded products.
    fn get_recorded_products(&self) -> Result<AdmittedForecastRecordedProducts, Self::Error> {
        self.calls.set(self.calls.get() + 1);
        Ok(AdmittedForecastRecordedProducts {
            day: "2026-11-23".to_owned(),
            forecast_observations: self.forecast_observations.clone(),
            generated_at: "2026-11-23T12:00:00.000Z".to_owned(),
            solar_cloud_evidence: self.solar_cloud_evidence.clone(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FixtureTileError;

impl Display for FixtureTileError {
    /// render the unused fixture provider failure.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("tile fixture failed")
    }
}

impl XweatherMapTileCacheError for FixtureTileError {
    /// report that this ordinary fixture never represents budget exhaustion.
    fn is_budget_exhausted(&self) -> bool {
        false
    }
}

#[derive(Debug, Clone, Copy)]
struct FixtureTileLoader;

impl XweatherTileLoader for FixtureTileLoader {
    type Error = FixtureTileError;

    /// return one non-empty admitted provider body.
    fn load_tile(
        &self,
        _request: &weather_edge::XweatherTileCacheRequest,
    ) -> Result<Vec<u8>, Self::Error> {
        Ok(b"png-fixture".to_vec())
    }
}

#[derive(Debug, Clone, Copy)]
struct FixtureTileClock;

impl XweatherTileClock for FixtureTileClock {
    /// return the same instant used by the admitted map request.
    fn now_epoch_ms(&self) -> i64 {
        MAP_NOW_EPOCH_MS
    }
}

#[derive(Debug, Default)]
struct FixtureScorecardClock;

impl ForecastAdjustmentScorecardClock for FixtureScorecardClock {
    type Error = Infallible;

    /// return one admitted scorecard read instant.
    fn now(&self) -> Result<AdmittedForecastAdjustmentScorecardNow, Self::Error> {
        Ok(AdmittedForecastAdjustmentScorecardNow {
            canonical: "2026-11-23T12:00:00.000Z".to_owned(),
            epoch_ms: 2_000,
        })
    }
}

#[derive(Debug, Default)]
struct FixtureScorecardStore;

impl ForecastAdjustmentScorecardStore for FixtureScorecardStore {
    type Error = Infallible;

    /// return one coherent current v2 publication.
    fn read_candidates(
        &self,
        _now: &AdmittedForecastAdjustmentScorecardNow,
    ) -> Result<ForecastAdjustmentScorecardCandidates, Self::Error> {
        Ok(ForecastAdjustmentScorecardCandidates {
            current_v2: Some(AdmittedForecastAdjustmentScorecardSnapshot::admitted(
                ForecastAdjustmentScorecardContractVersion::V2,
                1_000,
                AdmittedForecastAdjustmentScorecardJson::admitted(b"{\"score\":1}"),
                3_000,
            )),
            legacy: None,
            pending_v2: None,
            previous_v2_present: false,
        })
    }
}

#[derive(Debug, Default)]
struct FixtureScorecardEncoder;

impl ForecastAdjustmentScorecardEncoder for FixtureScorecardEncoder {
    type Error = Infallible;

    /// encode one already-admitted scorecard response fixture.
    fn encode_response(
        &self,
        _snapshot: &AdmittedForecastAdjustmentScorecardSnapshot,
        publication_state: ForecastAdjustmentScorecardPublicationState,
    ) -> Result<AdmittedForecastAdjustmentScorecardResponseJson, Self::Error> {
        assert_eq!(
            publication_state,
            ForecastAdjustmentScorecardPublicationState::Current
        );
        Ok(AdmittedForecastAdjustmentScorecardResponseJson::admitted(
            b"{\"data\":{\"score\":1},\"publicationState\":\"current\"}",
        ))
    }
}

struct Fixture {
    admin_login_renderer: FixtureAdminLoginRenderer,
    api: FixtureApi,
    assets: FixtureAssets,
    layout: InMemoryPropertySensorLayoutStore,
    layout_update_instants: FixturePropertyUpdateInstants,
    map_cache: XweatherTileMemoryCache<FixtureTileLoader, FixtureTileClock>,
    recorded: FixtureRecorded,
    renderer: FixtureRenderer,
    scorecard_clock: FixtureScorecardClock,
    scorecard_encoder: FixtureScorecardEncoder,
    scorecard_store: FixtureScorecardStore,
    settings: InMemoryForecastAdjustmentSettingsStore,
    widget_client: FixtureWidgetClient,
    widget_clock: FixtureWidgetClock,
    widget_projector: FixtureWidgetProjector,
}

#[derive(Debug, Clone)]
struct FixtureAdminLoginRenderer {
    body: Vec<u8>,
}

impl FixtureAdminLoginRenderer {
    /// replace the admitted source-rendered login body.
    fn set_body_length(&mut self, length: usize) {
        self.body = vec![b'l'; length];
    }
}

impl Default for FixtureAdminLoginRenderer {
    /// create one small admitted login representation.
    fn default() -> Self {
        Self {
            body: b"rendered login page".to_vec(),
        }
    }
}

impl AdminLoginPageRenderer for FixtureAdminLoginRenderer {
    type Error = Infallible;

    /// return one admitted login representation without implementing HTML substitution.
    fn render_login_page(
        &self,
        source: &AdmittedPublicAssetBytes,
        request: AdminLoginPageRenderRequest,
    ) -> Result<AdmittedPublicAssetBytes, Self::Error> {
        assert_eq!(source.as_bytes(), b"asset:admin-login.html");
        assert!(!request.invalid_credentials);
        Ok(AdmittedPublicAssetBytes::admitted(self.body.clone()))
    }
}

#[derive(Debug, Default)]
struct FixturePropertyUpdateInstants {
    calls: usize,
}

impl PropertySensorLayoutUpdateInstantSource for FixturePropertyUpdateInstants {
    type Error = Infallible;

    /// return one admitted canonical property update instant.
    fn admitted_update_instant(
        &mut self,
    ) -> Result<AdmittedPropertySensorUpdateInstant, Self::Error> {
        self.calls += 1;
        Ok(AdmittedPropertySensorUpdateInstant::admitted(
            "2026-11-23T12:34:56.000Z",
        ))
    }
}

#[derive(Debug, Default)]
struct FixtureAdminSession;

impl WeatherAdminSessionApplicationPort for FixtureAdminSession {
    /// reject accidental bootstrap use outside the dedicated lifecycle fixture.
    fn bootstrap(
        &mut self,
        _state: weather_edge::AdmittedAdminPersistentAuthState,
    ) -> Result<WeatherAdminBootstrapStatus, WeatherAdminStoreError> {
        unreachable!("ordinary route fixture does not dispatch bootstrap")
    }

    /// reject accidental login use outside the dedicated lifecycle fixture.
    fn login(
        &mut self,
        _credential: &weather_edge::AdmittedAdminCredential,
    ) -> Result<Option<WeatherAdminSessionResponse>, WeatherAdminStoreError> {
        unreachable!("ordinary route fixture does not dispatch login")
    }

    /// reject accidental logout use outside the dedicated lifecycle fixture.
    fn logout(
        &mut self,
        _token: Option<&weather_edge::AdmittedAdminSessionToken>,
    ) -> Result<(), WeatherAdminStoreError> {
        unreachable!("ordinary route fixture does not dispatch logout")
    }

    /// reject accidental page authentication outside the dedicated lifecycle fixture.
    fn authenticate(
        &mut self,
        _token: &weather_edge::AdmittedAdminSessionToken,
    ) -> Result<bool, WeatherAdminStoreError> {
        unreachable!("ordinary route fixture does not dispatch an admin page")
    }
}

type SessionTrace = Rc<RefCell<Vec<&'static str>>>;

struct FixtureAuthState {
    state: Option<AdmittedAdminPersistentAuthState>,
    trace: SessionTrace,
}

impl WeatherAdminAuthStatePort for FixtureAuthState {
    /// read the current optional admitted credential state.
    fn read_auth_state(
        &mut self,
    ) -> Result<Option<AdmittedAdminPersistentAuthState>, WeatherAdminPortError> {
        self.trace.borrow_mut().push("read-auth");
        Ok(self.state.clone())
    }

    /// retain the first admitted credential state.
    fn write_auth_state(
        &mut self,
        state: &AdmittedAdminPersistentAuthState,
    ) -> Result<(), WeatherAdminPortError> {
        self.trace.borrow_mut().push("write-auth");
        self.state = Some(state.clone());
        Ok(())
    }
}

struct FixtureAdminClock {
    now_epoch_milliseconds: i64,
    trace: SessionTrace,
}

impl WeatherAdminSessionClock for FixtureAdminClock {
    /// return one admitted lifecycle clock sample.
    fn now(&mut self) -> Result<AdmittedAdminSessionNow, WeatherAdminPortError> {
        self.trace.borrow_mut().push("clock");
        Ok(AdmittedAdminSessionNow {
            epoch_milliseconds: self.now_epoch_milliseconds,
        })
    }
}

struct FixtureAdminMaterial {
    next_ordinal: u64,
    trace: SessionTrace,
}

impl WeatherAdminSessionMaterialPort for FixtureAdminMaterial {
    /// issue one deterministic admitted fixture token.
    fn issue_token(&mut self) -> Result<AdmittedAdminSessionToken, WeatherAdminPortError> {
        self.trace.borrow_mut().push("token");
        self.next_ordinal += 1;
        let token = AdmittedAdminSessionToken::admitted(format!(
            "{}{:02}",
            "A".repeat(41),
            self.next_ordinal
        ));
        Ok(token)
    }

    /// return one admitted digest without implementing hashing.
    fn digest_token(
        &mut self,
        token: &AdmittedAdminSessionToken,
    ) -> Result<AdmittedAdminSessionDigest, WeatherAdminPortError> {
        self.trace.borrow_mut().push("digest");
        Ok(AdmittedAdminSessionDigest::admitted(format!(
            "fixture-digest:{}",
            token.expose()
        )))
    }
}

impl Fixture {
    /// create one complete no-host edge application fixture.
    fn new() -> Self {
        Self {
            admin_login_renderer: FixtureAdminLoginRenderer::default(),
            api: FixtureApi::default(),
            assets: FixtureAssets,
            layout: InMemoryPropertySensorLayoutStore::missing(),
            layout_update_instants: FixturePropertyUpdateInstants::default(),
            map_cache: XweatherTileMemoryCache::new(
                FixtureTileLoader,
                FixtureTileClock,
                1_024,
                60_000,
            )
            .expect("valid map cache fixture"),
            recorded: FixtureRecorded::default(),
            renderer: FixtureRenderer,
            scorecard_clock: FixtureScorecardClock,
            scorecard_encoder: FixtureScorecardEncoder,
            scorecard_store: FixtureScorecardStore,
            settings: InMemoryForecastAdjustmentSettingsStore::configured_first_read(),
            widget_client: FixtureWidgetClient::default(),
            widget_clock: FixtureWidgetClock,
            widget_projector: FixtureWidgetProjector,
        }
    }

    /// dispatch one request through every real typed application port.
    fn dispatch(&mut self, request: WeatherEdgeDispatchRequest<'_>) -> WeatherEdgeDispatchOutcome {
        let mut admin_session = FixtureAdminSession;
        self.dispatch_with_admin(request, &mut admin_session)
    }

    /// dispatch one request through an explicitly selected session application.
    fn dispatch_with_admin<Admin: WeatherAdminSessionApplicationPort>(
        &mut self,
        request: WeatherEdgeDispatchRequest<'_>,
        admin_session: &mut Admin,
    ) -> WeatherEdgeDispatchOutcome {
        dispatch_weather_edge_request(request, self.ports(admin_session))
    }

    /// Borrow the same application owners for both reply entry points.
    fn ports<'a, Admin: WeatherAdminSessionApplicationPort>(
        &'a mut self,
        admin_session: &'a mut Admin,
    ) -> impl WeatherEdgeDispatchPortSet + 'a {
        WeatherEdgeDispatchPorts {
            admin_login_renderer: &self.admin_login_renderer,
            admin_session,
            api: &self.api,
            assets: &self.assets,
            layout: &mut self.layout,
            layout_update_instants: &mut self.layout_update_instants,
            map_cache: &self.map_cache,
            property_sensor_center: PropertySensorMapCenter {
                latitude: 47.95,
                longitude: -122.43,
            },
            recorded: &self.recorded,
            renderer: &self.renderer,
            scorecard_clock: &self.scorecard_clock,
            scorecard_encoder: &self.scorecard_encoder,
            scorecard_store: &self.scorecard_store,
            settings: &mut self.settings,
            widget_client: &self.widget_client,
            widget_clock: &self.widget_clock,
            widget_projector: &self.widget_projector,
        }
    }
}

#[derive(Default)]
struct CapturedForecastRequests {
    prepared: Vec<(Vec<u8>, Option<ForecastEvidenceWindow>)>,
    commits: usize,
    closes: usize,
}

struct DispatchCaptureClock;

impl RevisionArchiveCanonicalClock for DispatchCaptureClock {
    /// Report the fixture's fixed time only from the response observer.
    fn canonical_now(&mut self) -> Option<String> {
        Some("2026-11-23T20:10:05.500Z".to_owned())
    }
}

struct DispatchCaptureArchive;

impl ForecastScheduledEvidenceArchive<()> for DispatchCaptureArchive {
    type Error = Infallible;

    /// An ordinary forecast response does not append scheduled archive evidence.
    fn append_committed(
        &mut self,
        _: &str,
        _: &(),
    ) -> Result<ForecastArchiveAppendOutcome, Infallible> {
        panic!("ordinary fixture has no archive append");
    }
}

#[derive(Default)]
struct FixtureWireWriter {
    head: Option<(u16, usize)>,
    body: Option<EdgeWireBody>,
    observer: Option<EdgeForecastTerminalObserver<()>>,
    trace: Vec<&'static str>,
}

impl EdgeWireWriter<()> for FixtureWireWriter {
    type Error = Infallible;

    /// Observe the selected metadata before handing a response to the body sender.
    fn write_head(
        &mut self,
        status_code: u16,
        _: PublicServingHeaderProfile,
        headers: &EdgeWireHeaders,
        _: Option<&EdgeWireAdminCookie>,
    ) -> Result<(), Infallible> {
        assert!(self.body.is_none());
        self.head = Some((status_code, headers.content_length));
        self.trace.push("head");
        Ok(())
    }

    /// Bind the callback only after headers, before the body sender runs.
    fn install_forecast_terminal(
        &mut self,
        observer: EdgeForecastTerminalObserver<()>,
    ) -> Result<(), Infallible> {
        assert!(self.head.is_some());
        assert!(self.body.is_none());
        assert!(observer.is_pending());
        assert!(self.observer.replace(observer).is_none());
        self.trace.push("observer");
        Ok(())
    }

    /// Take ownership without cloning an already encoded body or cached tile.
    fn send_body(&mut self, body: EdgeWireBody) -> Result<(), Infallible> {
        assert!(self.head.is_some());
        assert!(self.body.is_none());
        self.body = Some(body);
        self.trace.push("body");
        Ok(())
    }
}

#[derive(Default)]
struct FixtureWireJson {
    refuse_layout: bool,
}

impl EdgeWireJsonEncoder for FixtureWireJson {
    type Error = &'static str;

    /// Keep this fixture focused on layout and existing encoded routes.
    fn encode_adjustment_settings(
        &mut self,
        _: &ForecastAdjustmentSettingsEnvelope,
    ) -> Result<Vec<u8>, Self::Error> {
        panic!("unexpected settings encode")
    }

    /// Supply the exact ordinary empty layout response from the source encoder.
    fn encode_property_sensor_layout(
        &mut self,
        entries: &[PropertySensorLayoutEntry],
    ) -> Result<Vec<u8>, Self::Error> {
        assert!(entries.is_empty());
        // exercise the outer handler when response serialization cannot complete
        if self.refuse_layout {
            return Err("layout fixture cannot serialize");
        }
        Ok(b"{\"data\":[]}\n".to_vec())
    }

    /// This fixture never updates the recorded layout.
    fn encode_property_sensor_layout_entry(
        &mut self,
        _: &PropertySensorLayoutEntry,
    ) -> Result<Vec<u8>, Self::Error> {
        panic!("unexpected layout-entry encode")
    }

    /// This fixture never materializes viewer context.
    fn encode_viewer_context(&mut self, _: bool) -> Result<Vec<u8>, Self::Error> {
        panic!("unexpected viewer encode")
    }
}

impl ForecastResponseEvidenceStore for CapturedForecastRequests {
    type Prepared = ();
    type Error = Infallible;

    /// Record the exact public reply bytes available to the pre-send stage.
    fn prepare(
        &mut self,
        source: ForecastCaptureSource<'_>,
        window: Option<ForecastEvidenceWindow>,
        version: ForecastCaptureVersion,
    ) -> Option<()> {
        let ForecastCaptureSource::Encoded(bytes) = source else {
            panic!("expected encoded fixture forecast");
        };
        assert_eq!(version, ForecastCaptureVersion::Legacy);
        self.prepared.push((bytes.to_vec(), window));
        Some(())
    }

    /// Record only writes from the deferred post-response work.
    fn commit(
        &mut self,
        _: &(),
        committed_at: &str,
    ) -> Result<ForecastEvidenceOutcome, Infallible> {
        assert_eq!(committed_at, "2026-11-23T20:10:05.500Z");
        self.commits += 1;
        Ok(ForecastEvidenceOutcome::Created)
    }

    /// Count a disconnected ordinary response before finish.
    fn note_close_without_finish(&mut self) {
        self.closes += 1;
    }

    /// No storage failure is recorded before terminal completion.
    fn note_write_failure(&mut self) {}
}

/// build one request without optional private or map admissions.
fn request<'a>(
    method: &'a str,
    pathname: &'a str,
    search: &'a str,
) -> WeatherEdgeDispatchRequest<'a> {
    WeatherEdgeDispatchRequest {
        accept: None,
        admin_mutation: None,
        admin_operator: None,
        admin_route_input: None,
        adjustment_settings_body: None,
        has_query_delimiter: !search.is_empty(),
        map_request: None,
        method,
        pathname,
        property_sensor_mutation: None,
        remote_agents_preview: None,
        release: RELEASE,
        search,
        viewer_context: None,
    }
}
