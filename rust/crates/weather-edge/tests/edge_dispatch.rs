include!("support/edge_dispatch_fixture.rs");

/// Use the real public API dispatcher to arm evidence only on an ordinary forecast GET.
#[test]
fn captures_only_the_dispatched_successful_forecast_reply() {
    let mut fixture = Fixture::new();
    let mut admin = FixtureAdminSession;
    let mut store = CapturedForecastRequests::default();
    let mut json = FixtureWireJson::default();
    let forecast_path = "/api/v1/sites/ballydidean/forecast";
    let mut clock = DispatchCaptureClock;
    // use distinct ordinary windows and terminal events on the exact dispatched URL
    for (method, path, search, terminal) in [
        (
            "GET",
            forecast_path,
            "?days=1",
            Some(ForecastResponseTerminal::Finished),
        ),
        (
            "GET",
            forecast_path,
            "?days=10",
            Some(ForecastResponseTerminal::Closed),
        ),
        ("HEAD", forecast_path, "?days=1", None),
        ("GET", "/api/v1/sites/ballydidean/current", "?days=1", None),
    ] {
        let commits_before = store.commits;
        let reply = handle_edge_wire_request(
            request(method, path, search),
            fixture.ports(&mut admin),
            EdgeForecastCaptureAdmission {
                channel_bound_capture_active: false,
                scheduled_capture: None,
            },
            // the fixture decoder receives the original admitted search only after a successful get
            |search| match search {
                "?days=1" if terminal.is_some() => Some(ForecastEvidenceWindow::OneDay),
                "?days=10" if terminal.is_some() => Some(ForecastEvidenceWindow::TenDays),
                _ => panic!("unexpected evidence window decoding for {method} {path}{search}"),
            },
            &mut store,
            &mut json,
        )
        .expect("ordinary public application response");
        let mut writer = FixtureWireWriter::default();
        emit_edge_wire_response(reply, &mut writer).unwrap();
        assert_eq!(writer.head.unwrap().0, 200);
        assert_eq!(writer.observer.is_some(), terminal.is_some());
        // compare the writer's owned payload with what capture prepared from that response
        if let Some(terminal) = terminal {
            assert_eq!(writer.trace, ["head", "observer", "body"]);
            let mut observer = writer.observer.take().unwrap();
            let EdgeWireBody::Owned(body) = writer.body.unwrap() else {
                panic!("expected the owned forecast bytes");
            };
            assert_eq!(body, store.prepared.last().unwrap().0);
            drop(body);
            let work = observer.observe(terminal, &mut clock, &mut store).unwrap();
            assert!(!observer.is_pending());
            assert_eq!(store.commits, commits_before);
            let finished = terminal == ForecastResponseTerminal::Finished;
            let later = if finished {
                ForecastResponseTerminal::Closed
            } else {
                ForecastResponseTerminal::Finished
            };
            assert!(observer.observe(later, &mut clock, &mut store).is_none());
            let completed = persist_forecast_response_capture(
                work,
                &mut store,
                ForecastScheduleArchive::<DispatchCaptureArchive>::Disabled,
            );
            assert_eq!(
                completed.evidence,
                if finished {
                    ForecastEvidenceOutcome::Created
                } else {
                    ForecastEvidenceOutcome::ResponseAborted
                }
            );
        } else {
            assert_eq!(writer.trace, ["head", "body"]);
        }
    }
    assert_eq!(fixture.api.calls.get(), 4);
    assert_eq!(store.prepared.len(), 2);
    assert_eq!(store.prepared[0].1, Some(ForecastEvidenceWindow::OneDay));
    assert_eq!(store.prepared[1].1, Some(ForecastEvidenceWindow::TenDays));
    assert_eq!(store.commits, 1);
    assert_eq!(store.closes, 1);
}

/// retain admitted recorded allocations through both get writers and suppress head bodies.
#[test]
fn emits_recorded_products_from_their_admitted_allocations() {
    let mut fixture = Fixture::new();
    let mut admin = FixtureAdminSession;
    let mut store = CapturedForecastRequests::default();
    let mut json = FixtureWireJson::default();
    let products = [
        (
            "/api/v1/sites/ballydidean/forecast-observations",
            b"{\"data\":[]}".as_slice(),
            fixture.recorded.forecast_observations.as_bytes().as_ptr(),
        ),
        (
            "/api/v1/sites/ballydidean/solar-cloud-evidence",
            b"{\"evidence\":{}}".as_slice(),
            fixture.recorded.solar_cloud_evidence.as_bytes().as_ptr(),
        ),
    ];

    // exercise both recorded representations through the real dispatch and writer boundaries
    for (path, expected_body, admitted_pointer) in products {
        // preserve get metadata on head while suppressing only its emitted body
        for (method, expected_pointer) in [("GET", Some(admitted_pointer)), ("HEAD", None)] {
            let reply = handle_edge_wire_request(
                request(method, path, ""),
                fixture.ports(&mut admin),
                EdgeForecastCaptureAdmission {
                    channel_bound_capture_active: false,
                    scheduled_capture: None,
                },
                |_| panic!("recorded route decoded a forecast evidence window"),
                &mut store,
                &mut json,
            )
            .expect("ordinary recorded-product wire response");
            let response = reply.response();
            assert_eq!(response.status_code, 200);
            assert_eq!(
                response.header_profile,
                PublicServingHeaderProfile::Standard
            );
            assert_eq!(response.admin_cookie, None);
            assert_eq!(
                &response.headers,
                &EdgeWireHeaders {
                    allow: None,
                    cache_control: "public, max-age=60, s-maxage=60".into(),
                    content_length: expected_body.len(),
                    content_type: Some("application/json; charset=utf-8".into()),
                    location: None,
                    retry_after: None,
                    tile_age_seconds: None,
                    tile_cache: None,
                    vary: None,
                }
            );

            let mut writer = FixtureWireWriter::default();
            emit_edge_wire_response(reply, &mut writer).unwrap();
            assert_eq!(writer.head, Some((200, expected_body.len())));
            assert_eq!(writer.trace, ["head", "body"]);
            match expected_pointer {
                Some(expected_pointer) => {
                    let emitted = writer.body.as_ref().expect("recorded get body");
                    assert_eq!(emitted.as_bytes(), expected_body);
                    assert_eq!(emitted.as_bytes().as_ptr(), expected_pointer);
                }
                None => assert_eq!(writer.body, Some(EdgeWireBody::Empty)),
            }
        }
    }
    assert_eq!(fixture.recorded.calls.get(), 4);
}

/// Reach the same writer from a cached tile and a JSON-only layout representation.
#[test]
fn materializes_map_layout_and_head_failures_for_wire_output() {
    let mut fixture = Fixture::new();
    let mut admin = FixtureAdminSession;
    let mut store = CapturedForecastRequests::default();
    let mut json = FixtureWireJson::default();
    let mut map = request(
        "GET",
        "/maps/xweather/history/clouds/2026-08-28T12:00:00.000Z/10/47.95/-122.43/256/168.png",
        "",
    );
    map.map_request = Some(map_request());
    // run real typed dispatch without a terminal observer for non-forecast requests
    let mut wire = |request, json: &mut FixtureWireJson| {
        handle_edge_wire_request(
            request,
            fixture.ports(&mut admin),
            EdgeForecastCaptureAdmission {
                channel_bound_capture_active: false,
                scheduled_capture: None,
            },
            |_| panic!("non-forecast route decoded evidence window"),
            &mut store,
            json,
        )
        .expect("ordinary materialized wire response")
    };
    let map = wire(map, &mut json);
    let map = map.response();
    assert_eq!(map.status_code, 200);
    assert_eq!(map.headers.content_type.as_deref(), Some("image/png"));
    assert_eq!(map.headers.tile_age_seconds, Some(0));
    assert!(map.headers.tile_cache.is_some());
    assert_eq!(map.headers.content_length, map.body.as_bytes().len());
    assert!(matches!(&map.body, EdgeWireBody::Tile(_)));

    let layout_path = "/api/v1/sites/ballydidean/property-sensor-layout";
    let layout = wire(request("HEAD", layout_path, ""), &mut json);
    let layout = layout.response();
    assert_eq!(layout.status_code, 200);
    assert_eq!(layout.headers.content_length, b"{\"data\":[]}\n".len());
    assert_eq!(layout.headers.cache_control, "no-cache");
    assert!(layout.body.is_empty());

    json.refuse_layout = true;
    let failed = wire(request("HEAD", layout_path, ""), &mut json);
    let mut writer = FixtureWireWriter::default();
    emit_edge_wire_response(failed, &mut writer).unwrap();
    assert_eq!(writer.head, Some((500, b"internal server error\n".len())));
    assert_eq!(writer.body, Some(EdgeWireBody::Empty));

    // use the same materialization path for an admitted admin method response
    let admin = wire(request("GET", "/admin/logout", ""), &mut json);
    assert_eq!(admin.response().headers.allow, Some("POST"));
    let mut writer = FixtureWireWriter::default();
    emit_edge_wire_response(admin, &mut writer).unwrap();
    assert_eq!(writer.head, Some((405, b"method not allowed\n".len())));
    assert_eq!(writer.body.unwrap().as_bytes(), b"method not allowed\n");
    assert_eq!(writer.trace, ["head", "body"]);
}

/// Preserve the normal admitted administrator's personalized public settings representation.
#[test]
fn admitted_admin_read_keeps_public_html_personalized_without_varying_assets() {
    let mut fixture = Fixture::new();
    let operator = AdmittedAdminOperator::admitted("ordinary fixture operator");
    let mut settings = request("GET", "/settings", "");
    settings.admin_operator = Some(&operator);
    let WeatherEdgeDispatchOutcome::Public(response) = fixture.dispatch(settings) else {
        panic!("expected the ordinary settings representation");
    };
    assert_eq!(response.status_code, 200);
    assert_eq!(response.headers.cache_control, "private, no-store");
    assert_eq!(response.headers.vary, Some("Cookie"));
    assert_eq!(
        response.body,
        PublicServingApplicationBody::Bytes(Some(b"admitted settings".to_vec()))
    );

    let mut worker = request("GET", "/service-worker.js", "");
    worker.admin_operator = Some(&operator);
    let WeatherEdgeDispatchOutcome::Public(response) = fixture.dispatch(worker) else {
        panic!("expected the ordinary worker representation");
    };
    assert_eq!(response.headers.cache_control, "no-store");
    assert_eq!(response.headers.vary, None);
}

/// build one decoded ordinary map request before top-level method orientation.
fn map_request() -> XweatherMapProxyRequest {
    XweatherMapProxyRequest {
        method: XweatherMapProxyMethod::Unsupported,
        now_epoch_ms: MAP_NOW_EPOCH_MS,
        provider: Some(AdmittedXweatherProvider::admitted()),
        retry_after_utc_day_seconds: "3600".to_owned(),
        route: DecodedXweatherMapRoute::Frame(DecodedXweatherFrameRoute {
            cache_key: XweatherTileCacheKey::admitted("history-clouds-fixture"),
            height: 168,
            latitude: 47.95,
            latitude_millionths: 47_950_000,
            layer: "clouds".to_owned(),
            longitude: -122.43,
            longitude_millionths: -122_430_000,
            phase: "history".to_owned(),
            valid_time: Some(AdmittedXweatherValidTime::admitted(
                "2026-08-28T12:00:00.000Z",
                MAP_NOW_EPOCH_MS,
                0,
                0,
            )),
            width: 256,
            zoom: 10,
        }),
        site: AdmittedXweatherMapSite {
            latitude_millionths: 47_950_000,
            longitude_millionths: -122_430_000,
        },
    }
}

// verifies real specialized handlers retain priority over the general API proxy
#[test]
fn dispatches_ordinary_routes_through_their_existing_application_owners() {
    let mut fixture = Fixture::new();

    let settings = fixture.dispatch(request(
        "GET",
        "/api/v1/sites/ballydidean/forecast-adjustment-settings",
        "",
    ));
    assert!(matches!(
        settings,
        WeatherEdgeDispatchOutcome::AdjustmentSettings(
            ForecastAdjustmentSettingsOutcome::Success {
                omit_body: false,
                ..
            }
        )
    ));
    assert_eq!(fixture.api.calls.get(), 0);

    let layout = fixture.dispatch(request(
        "HEAD",
        "/api/v1/sites/ballydidean/property-sensor-layout",
        "",
    ));
    assert!(matches!(
        layout,
        WeatherEdgeDispatchOutcome::PropertySensorLayout(
            PropertySensorLayoutOutcome::ReadSuccess {
                omit_body: true,
                ..
            }
        )
    ));

    let widget = fixture.dispatch(request(
        "HEAD",
        "/api/v3/sites/ballydidean/widget-forecast",
        "",
    ));
    assert!(matches!(
        widget,
        WeatherEdgeDispatchOutcome::Widget(response)
            if response.status_code == 200
                && matches!(response.body, WidgetForecastApplicationBody::Json(None))
    ));
    assert_eq!(fixture.widget_client.calls.get(), 1);

    let recorded = fixture.dispatch(request(
        "GET",
        "/api/v1/sites/ballydidean/forecast-observations",
        "",
    ));
    assert!(matches!(
        recorded,
        WeatherEdgeDispatchOutcome::Recorded(response)
            if response.status_code == 200
                && matches!(response.body, ForecastRecordedProductBody::Json(Some(_)))
    ));
    assert_eq!(fixture.recorded.calls.get(), 1);

    let mut map = request(
        "GET",
        "/maps/xweather/history/clouds/2026-08-28T12:00:00.000Z/10/47.95/-122.43/256/168.png",
        "",
    );
    map.map_request = Some(map_request());
    let map = fixture.dispatch(map);
    assert!(matches!(
        map,
        WeatherEdgeDispatchOutcome::Map(response)
            if response.status_code == 200
                && matches!(response.body, XweatherMapProxyBody::Png(Some(_)))
    ));

    let api = fixture.dispatch(request(
        "HEAD",
        "/api/v1/sites/ballydidean/current",
        "?station=yard",
    ));
    assert!(matches!(
        api,
        WeatherEdgeDispatchOutcome::Public(response)
            if response.status_code == 200
                && matches!(response.body, PublicServingApplicationBody::Bytes(None))
    ));
    assert_eq!(fixture.api.calls.get(), 1);

    let static_asset = fixture.dispatch(request("HEAD", "/brand/favicon.svg", ""));
    assert!(matches!(
        static_asset,
        WeatherEdgeDispatchOutcome::Public(response)
            if response.status_code == 200
                && matches!(response.body, PublicServingApplicationBody::Bytes(None))
    ));

    let mut preview = request(
        "GET",
        "/__rac/browser-device",
        "?mode=desktop&location=%2Fforecast%3Fdays%3D2%23hour-1",
    );
    preview.remote_agents_preview = Some(AdmittedRemoteAgentsPreviewTarget::admitted(
        RemoteAgentsPreviewMode::Desktop,
        "/forecast?days=2#hour-1",
    ));
    assert!(matches!(
        fixture.dispatch(preview),
        WeatherEdgeDispatchOutcome::RemoteAgentsPreview(response)
            if response.status_code == 302
                && response.cache_control == "no-store"
                && response.content_length == 0
                && response.location == "/forecast?days=2#hour-1"
    ));
}

// verifies admitted ordinary administrator requests use the existing typed handlers
#[test]
fn dispatches_admitted_admin_requests_without_owning_session_or_crypto_admission() {
    let mut fixture = Fixture::new();
    let operator = AdmittedAdminOperator::admitted("fixture-operator");

    let mut settings_request = request(
        "HEAD",
        "/api/v1/admin/sites/ballydidean/forecast-adjustment-settings",
        "",
    );
    settings_request.admin_operator = Some(&operator);
    let settings = fixture.dispatch(settings_request);
    assert!(matches!(
        settings,
        WeatherEdgeDispatchOutcome::AdjustmentSettings(
            ForecastAdjustmentSettingsOutcome::Success {
                omit_body: true,
                ..
            }
        )
    ));

    let mut replace_request = request(
        "PUT",
        "/api/v1/admin/sites/ballydidean/forecast-adjustment-settings",
        "",
    );
    replace_request.admin_mutation = Some(AdmittedAdminMutation::admitted(&operator));
    replace_request.adjustment_settings_body = Some(DecodedForecastAdjustmentSettings {
        field_count: 4,
        version: Some(1),
        temperature: Some(false),
        wind: Some(true),
        rain: Some(false),
    });
    let replace = fixture.dispatch(replace_request);
    assert!(matches!(
        replace,
        WeatherEdgeDispatchOutcome::AdjustmentSettings(
            ForecastAdjustmentSettingsOutcome::Success { envelope, .. }
        ) if !envelope.data.temperature && envelope.data.wind && !envelope.data.rain
    ));

    let mut property_request = request(
        "PUT",
        "/api/v1/admin/sites/ballydidean/property-sensor-layout/soil-1",
        "",
    );
    property_request.property_sensor_mutation = Some(AdmittedPropertySensorMutation {
        input: DecodedPropertySensorLayoutUpdate {
            display_name: Some("  Orchard soil  ".to_owned()),
            icon: Some("temperature".to_owned()),
            latitude: Some(47.9505),
            longitude: Some(-122.4281),
        },
        mutation: AdmittedAdminMutation::admitted(&operator),
        sensor_key: "soil-1".to_owned(),
    });
    let property = fixture.dispatch(property_request);
    let WeatherEdgeDispatchOutcome::PropertySensorLayout(property) = property else {
        panic!("unexpected property mutation outcome")
    };
    assert_eq!(property.status_code(), 200);
    assert_eq!(property.cache_control(), "no-store");
    let PropertySensorLayoutOutcome::UpdateSuccess { data } = property else {
        panic!("expected successful property mutation")
    };
    assert_eq!(data.sensor_key, "soil-1");
    assert_eq!(data.display_name, "Orchard soil");
    assert_eq!(data.icon.map(|icon| icon.wire_value()), Some("temperature"));
    assert_eq!(data.latitude, 47.9505);
    assert_eq!(data.longitude, -122.4281);
    assert_eq!(data.updated_at.as_str(), "2026-11-23T12:34:56.000Z");
    assert_eq!(fixture.layout_update_instants.calls, 1);

    let persisted = fixture.dispatch(request(
        "GET",
        "/api/v1/sites/ballydidean/property-sensor-layout",
        "",
    ));
    assert!(matches!(
        persisted,
        WeatherEdgeDispatchOutcome::PropertySensorLayout(
            PropertySensorLayoutOutcome::ReadSuccess { data, .. }
        ) if data.len() == 1 && data[0].sensor_key == "soil-1"
    ));

    let mut scorecard_request = request(
        "GET",
        "/api/v1/admin/sites/ballydidean/forecast-adjustment-scorecard",
        "",
    );
    scorecard_request.admin_operator = Some(&operator);
    let scorecard = fixture.dispatch(scorecard_request);
    assert!(matches!(
        scorecard,
        WeatherEdgeDispatchOutcome::AdjustmentScorecard(response)
            if response.status_code == 200
    ));
    assert_eq!(fixture.api.calls.get(), 0);
}

// verifies the dispatcher composes the real admitted bootstrap, login, viewer, and logout lifecycle
#[test]
fn dispatches_the_normal_admitted_admin_session_lifecycle() {
    let oracle =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/admin-edge-dispatch-node-oracle.mjs");
    let output = Command::new("node")
        .arg(oracle)
        .output()
        .expect("run admin edge Node oracle");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("Node oracle UTF-8"),
        concat!(
            "bootstrap=201|no-store|configured\n",
            "login=303|/admin|no-store|secure-partitioned|43200\n",
            "logout=303|/admin|no-store|secure-partitioned\n",
        )
    );

    let trace = Rc::new(RefCell::new(Vec::new()));
    let credential_digest = AdmittedAdminCredentialDigest::admitted("fixture-credential-v1");
    let persistent_state = AdmittedAdminPersistentAuthState::admitted(
        credential_digest.clone(),
        AdmittedAdminPasswordVerifier::admitted(vec![1; 64], vec![2; 16]),
    );
    let credential = AdmittedAdminCredential::verified(credential_digest);
    let mut auth = FixtureAuthState {
        state: None,
        trace: Rc::clone(&trace),
    };
    let mut clock = FixtureAdminClock {
        now_epoch_milliseconds: 1_795_452_400_000,
        trace: Rc::clone(&trace),
    };
    let mut material = FixtureAdminMaterial {
        next_ordinal: 0,
        trace: Rc::clone(&trace),
    };
    let mut store = WeatherAdminStore::new();
    let mut fixture = Fixture::new();

    let mut bootstrap_request = request("POST", "/api/v1/admin/bootstrap", "");
    bootstrap_request.admin_route_input =
        Some(AdmittedAdminRouteInput::Bootstrap(persistent_state));
    let bootstrap = {
        let mut admin =
            WeatherAdminSessionApplication::new(&mut store, &mut auth, &mut clock, &mut material);
        fixture.dispatch_with_admin(bootstrap_request, &mut admin)
    };
    assert!(matches!(
        bootstrap,
        WeatherEdgeDispatchOutcome::Admin(WeatherAdminRouteOutcome::BootstrapConfigured)
    ));
    assert_eq!(*trace.borrow(), ["read-auth", "write-auth"]);

    trace.borrow_mut().clear();
    let mut login_request = request("POST", "/admin/login", "");
    login_request.admin_route_input = Some(AdmittedAdminRouteInput::Login {
        cookie_mode: AdmittedAdminCookieMode::SecurePartitioned,
        credential,
    });
    let login = {
        let mut admin =
            WeatherAdminSessionApplication::new(&mut store, &mut auth, &mut clock, &mut material);
        fixture.dispatch_with_admin(login_request, &mut admin)
    };
    let token = match login {
        WeatherEdgeDispatchOutcome::Admin(WeatherAdminRouteOutcome::LoginRedirect {
            cookie_mode,
            session,
        }) => {
            assert_eq!(cookie_mode, AdmittedAdminCookieMode::SecurePartitioned);
            assert_eq!(session.maximum_age_seconds, ADMIN_SESSION_TTL_SECONDS);
            session.token
        }
        outcome => panic!("unexpected login outcome: {outcome:?}"),
    };
    assert_eq!(*trace.borrow(), ["read-auth", "clock", "token", "digest"]);

    let mut viewer_get = request("GET", "/api/v1/viewer-context", "");
    viewer_get.viewer_context = Some(AdmittedViewerContext { home_network: true });
    assert!(matches!(
        fixture.dispatch(viewer_get),
        WeatherEdgeDispatchOutcome::ViewerContext(WeatherViewerContextOutcome::Success {
            home_network: true,
            omit_body: false,
        })
    ));
    let mut viewer_head = request("HEAD", "/api/v1/viewer-context", "");
    viewer_head.viewer_context = Some(AdmittedViewerContext { home_network: true });
    assert!(matches!(
        fixture.dispatch(viewer_head),
        WeatherEdgeDispatchOutcome::ViewerContext(WeatherViewerContextOutcome::Success {
            home_network: true,
            omit_body: true,
        })
    ));

    trace.borrow_mut().clear();
    let mut admin_page_get = request("GET", "/admin", "");
    admin_page_get.admin_route_input = Some(AdmittedAdminRouteInput::Page {
        login: AdmittedAdminLoginPage {
            invalid_credentials: false,
        },
        token: Some(&token),
    });
    let admin_page_get = {
        let mut admin =
            WeatherAdminSessionApplication::new(&mut store, &mut auth, &mut clock, &mut material);
        fixture.dispatch_with_admin(admin_page_get, &mut admin)
    };
    assert!(matches!(
        admin_page_get,
        WeatherEdgeDispatchOutcome::Public(response)
            if response.status_code == 200
                && response.headers.cache_control == "private, no-store"
                && response.headers.vary == Some("Cookie")
                && matches!(response.body, PublicServingApplicationBody::Bytes(Some(_)))
    ));
    assert_eq!(*trace.borrow(), ["digest", "clock"]);

    trace.borrow_mut().clear();
    let mut admin_page_head = request("HEAD", "/admin", "");
    admin_page_head.admin_route_input = Some(AdmittedAdminRouteInput::Page {
        login: AdmittedAdminLoginPage {
            invalid_credentials: false,
        },
        token: Some(&token),
    });
    let admin_page_head = {
        let mut admin =
            WeatherAdminSessionApplication::new(&mut store, &mut auth, &mut clock, &mut material);
        fixture.dispatch_with_admin(admin_page_head, &mut admin)
    };
    assert!(matches!(
        admin_page_head,
        WeatherEdgeDispatchOutcome::Public(response)
            if response.status_code == 200
                && response.headers.cache_control == "private, no-store"
                && response.headers.vary == Some("Cookie")
                && matches!(response.body, PublicServingApplicationBody::Bytes(None))
    ));
    assert_eq!(*trace.borrow(), ["digest", "clock"]);

    trace.borrow_mut().clear();
    let mut logout_request = request("POST", "/admin/logout", "");
    logout_request.admin_route_input = Some(AdmittedAdminRouteInput::Logout {
        cookie_mode: AdmittedAdminCookieMode::SecurePartitioned,
        token: Some(&token),
    });
    let logout = {
        let mut admin =
            WeatherAdminSessionApplication::new(&mut store, &mut auth, &mut clock, &mut material);
        fixture.dispatch_with_admin(logout_request, &mut admin)
    };
    assert!(matches!(
        logout,
        WeatherEdgeDispatchOutcome::Admin(WeatherAdminRouteOutcome::LogoutRedirect {
            cookie_mode: AdmittedAdminCookieMode::SecurePartitioned,
        })
    ));
    assert_eq!(*trace.borrow(), ["digest"]);
}

// verifies the normal absent-session login document and bootstrap read method responses
#[test]
fn dispatches_normal_login_and_bootstrap_read_representations() {
    let oracle =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/admin-login-page-node-oracle.mjs");
    let output = Command::new("node")
        .arg(oracle)
        .output()
        .expect("run admin login page Node oracle");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let node = String::from_utf8(output.stdout).expect("Node oracle UTF-8");
    let normal_length = node
        .lines()
        .find_map(|line| line.strip_prefix("login-normal-length="))
        .expect("normal login length")
        .parse::<usize>()
        .expect("normal login length integer");
    assert!(node.contains(&format!(
        "login-get=200|no-store|text/html; charset=utf-8|Cookie|{normal_length}|body\n"
    )));
    assert!(node.contains(&format!(
        "login-head=200|no-store|text/html; charset=utf-8|Cookie|{normal_length}|none\n"
    )));
    assert!(node.contains("bootstrap-get=405|POST|no-store|method not allowed\n"));
    assert!(node.contains("bootstrap-head=405|POST|no-store|method not allowed\n"));

    let mut fixture = Fixture::new();
    fixture.admin_login_renderer.set_body_length(normal_length);
    let mut login_get = request("GET", "/admin", "");
    login_get.admin_route_input = Some(AdmittedAdminRouteInput::Page {
        login: AdmittedAdminLoginPage {
            invalid_credentials: false,
        },
        token: None,
    });
    let login_get = fixture.dispatch(login_get);
    assert!(matches!(
        login_get,
        WeatherEdgeDispatchOutcome::Public(response)
            if response.status_code == 200
                && response.headers.cache_control == "no-store"
                && response.headers.content_length == normal_length
                && response.headers.content_type == "text/html; charset=utf-8"
                && response.headers.vary == Some("Cookie")
                && matches!(&response.body, PublicServingApplicationBody::Bytes(Some(body)) if body.len() == normal_length)
    ));

    let mut login_head = request("HEAD", "/admin", "");
    login_head.admin_route_input = Some(AdmittedAdminRouteInput::Page {
        login: AdmittedAdminLoginPage {
            invalid_credentials: false,
        },
        token: None,
    });
    let login_head = fixture.dispatch(login_head);
    assert!(matches!(
        login_head,
        WeatherEdgeDispatchOutcome::Public(response)
            if response.status_code == 200
                && response.headers.content_length == normal_length
                && matches!(response.body, PublicServingApplicationBody::Bytes(None))
    ));

    // preserve the production method response before any bootstrap admission input
    for method in ["GET", "HEAD"] {
        let bootstrap = fixture.dispatch(request(method, "/api/v1/admin/bootstrap", ""));
        let WeatherEdgeDispatchOutcome::Admin(outcome) = bootstrap else {
            panic!("unexpected bootstrap read outcome")
        };
        assert_eq!(outcome.status_code(), 405);
        assert_eq!(outcome.cache_control(), "no-store");
        assert!(matches!(
            outcome,
            WeatherAdminRouteOutcome::MethodNotAllowed { allow }
                if allow.header_value() == "POST"
        ));
    }
}

// verifies the production callback retains specialized-before-proxy source priority
#[test]
fn matches_node_top_level_route_priority_source_vm() {
    let oracle =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/edge-dispatch-node-oracle.mjs");
    let output = Command::new("node")
        .arg(oracle)
        .output()
        .expect("run edge dispatch Node oracle");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("Node oracle UTF-8"),
        concat!(
            "public-settings=settings:public\n",
            "admin-settings=settings:admin\n",
            "widget=widget\n",
            "recorded=recorded\n",
            "api=api\n",
            "map=map\n",
            "static=static\n",
            "viewer-get=viewer|200|private, no-store|body\n",
            "viewer-head=viewer|200|private, no-store|none\n",
            "property-update=layout-update\n",
            "preview=preview\n",
            "admin-page-get=admin-auth,admin-read,admin-render:true:false|200|private, no-store|Cookie|body\n",
            "admin-page-head=admin-auth,admin-read,admin-render:true:false|200|private, no-store|Cookie|none\n",
        )
    );
}
