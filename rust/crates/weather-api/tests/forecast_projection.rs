use weather_adjustment::{WindCanaryAuthorizationWindow, apply_core_adjustment};
use weather_api::{
    AdmittedWindCanaryEvidence, AdmittedWindCanaryRuntime, AdmittedWindCorrections,
    AdmittedWindRowOutput, CanonicalInstant, CurrentProjectionError, CurrentTimeResolver,
    InactiveForecastDecisionConfig, group_active_sites, project_forecast_weather_records,
    project_forecast_weather_records_with_wind_adjustments, project_wind_canary_runtime,
};
use weather_contracts::{
    ApiForecastAdjustmentDecision, ApiForecastAdjustmentRuntimeActivationModeEnum,
    ApiForecastAdjustmentRuntimeReasonCodeEnum, ApiForecastAdjustmentRuntimeStateEnum,
    ApiRainAdjustmentRuntimeReasonCodeEnum, CoreAdjustmentApplicationInput,
    CoreAdjustmentApplicationInputEnvelope, CoreAdjustmentApplicationResult, FieldPresence,
    ForecastAdjustmentDecision, ForecastAdjustmentFailRawDecisionStateEnum,
    ForecastAdjustmentMetric, ForecastAdjustmentMetricBand,
    ForecastAdjustmentRawForecastProvenance, ForecastAdjustmentRawForecastProvenanceCohortLiteral,
    ForecastAdjustmentRawForecastProvenanceReferenceKindLiteral, ForecastAdjustmentReasonCode,
    ForecastAdjustmentWindCanaryMetric, ForecastLeadBandKey,
    ForecastTemperatureCanaryDecisionV1StateEnum, JsDateOrString, JsonValue, Nullable, SourceKind,
    StationKind, TemperatureCanaryDecisionReasonValue, WeatherRecordRow,
};
use weather_database::ActiveSiteRow;

/// explicit source-to-canonical time fixture
struct FixtureTimeResolver {
    values: Vec<(JsDateOrString, CanonicalInstant)>,
}

impl CurrentTimeResolver for FixtureTimeResolver {
    /// resolve one admitted fixture timestamp
    fn resolve(&self, value: &JsDateOrString) -> Result<CanonicalInstant, CurrentProjectionError> {
        self.values
            .iter()
            .find_map(|(source, canonical)| (source == value).then(|| canonical.clone()))
            .ok_or_else(|| CurrentProjectionError {
                code: "forecast_projection.time",
                detail: "fixture time adapter rejected the source timestamp".to_owned(),
            })
    }
}

/// encode one generated string
fn text(value: &str) -> Vec<u16> {
    value.encode_utf16().collect()
}

/// build one ordered object entry
fn entry(key: &str, value: JsonValue) -> (Vec<u16>, JsonValue) {
    (text(key), value)
}

/// build one admitted canonical instant
fn instant(epoch_milliseconds: i64, utc: &str) -> CanonicalInstant {
    CanonicalInstant {
        epoch_milliseconds,
        utc: utc.to_owned(),
    }
}

/// build node-derived source timestamp pairs
fn time_resolver() -> FixtureTimeResolver {
    FixtureTimeResolver {
        values: vec![
            (
                JsDateOrString::String(text("2026-08-22T05:00:00.000Z")),
                instant(1_787_374_800_000, "2026-08-22T05:00:00.000Z"),
            ),
            (
                JsDateOrString::String(text("2026-08-22T06:00:00.000Z")),
                instant(1_787_378_400_000, "2026-08-22T06:00:00.000Z"),
            ),
            (
                JsDateOrString::String(text("2026-08-22T07:00:00.000Z")),
                instant(1_787_382_000_000, "2026-08-22T07:00:00.000Z"),
            ),
            (
                JsDateOrString::String(text("2026-09-08T18:00:00.000Z")),
                instant(1_788_890_400_000, "2026-09-08T18:00:00.000Z"),
            ),
            (
                JsDateOrString::String(text("2026-09-09T05:00:00.000Z")),
                instant(1_788_930_000_000, "2026-09-09T05:00:00.000Z"),
            ),
            (
                JsDateOrString::String(text("2026-09-09T06:00:00.000Z")),
                instant(1_788_933_600_000, "2026-09-09T06:00:00.000Z"),
            ),
        ],
    }
}

/// build one active forecast source row
fn active_source(source_id: &str) -> ActiveSiteRow {
    ActiveSiteRow {
        attribution_label: "Weather data by Open-Meteo".to_owned(),
        attribution_url: "https://open-meteo.com/".to_owned(),
        latitude: 47.95,
        longitude: -122.42,
        provider_key: "open-meteo".to_owned(),
        provider_name: "Open-Meteo".to_owned(),
        site_name: "Ballydidean".to_owned(),
        site_slug: "ballydidean".to_owned(),
        source_id: source_id.to_owned(),
        source_key: format!("forecast-{source_id}"),
        source_kind: SourceKind::Forecast,
        station_kind: StationKind::Virtual,
        station_latitude: 47.96,
        station_longitude: -122.43,
        station_name: "Open-Meteo".to_owned(),
        station_slug: "open-meteo-virtual".to_owned(),
        timezone: "America/Los_Angeles".to_owned(),
    }
}

/// build one representative forecast row
fn forecast_row(source_id: &str, valid_at: &str) -> WeatherRecordRow {
    WeatherRecordRow {
        adapterVersion: Nullable::Value("forecast-daily/v4".to_owned()),
        apparentTemperatureC: Nullable::Value(11.7),
        blackGlobeTemperatureC: Nullable::Null,
        cloudCoverPercent: Nullable::Value(72.0),
        contentHash: "sha256-content".to_owned(),
        contractEpoch: Nullable::Value("v4".to_owned()),
        deviceModel: Nullable::Null,
        deviceSerial: Nullable::Null,
        deviceVendor: Nullable::Null,
        firstReceivedAt: JsDateOrString::String(text("2026-08-22T05:00:00.000Z")),
        id: format!("forecast-record-{source_id}"),
        lastReceivedAt: JsDateOrString::String(text("2026-08-22T05:00:00.000Z")),
        pm25MicrogramsPerCubicMeter: Nullable::Value(5.0),
        precipitationMm: Nullable::Value(0.1),
        precipitationRateMmPerHour: Nullable::Value(0.1),
        pressureHpa: Nullable::Value(1018.1),
        productRunAt: Nullable::Value(JsDateOrString::String(text("2026-08-22T05:00:00.000Z"))),
        providerKey: "ignored-row-provider".to_owned(),
        providerMetadata: Nullable::Value(vec![
            entry("dataset", JsonValue::String(text("forecast"))),
            entry("elevation_m", JsonValue::Number(32.0)),
            entry("grid_cell", JsonValue::String(text("47.941,-122.438"))),
            entry(
                "property_sensors",
                JsonValue::Array(vec![JsonValue::Object(vec![
                    entry("channel", JsonValue::Number(1.0)),
                    entry("key", JsonValue::String(text("temperature-1"))),
                    entry("model", JsonValue::String(text("WN31"))),
                    entry(
                        "readings",
                        JsonValue::Object(vec![entry("temperatureC", JsonValue::Number(20.0))]),
                    ),
                ])]),
            ),
        ]),
        qualityMetadata: Nullable::Null,
        relativeHumidityPercent: Nullable::Value(83.0),
        revisionCount: 0,
        siteSlug: "ballydidean".to_owned(),
        sourceId: source_id.to_owned(),
        sourceConfigFingerprint: Nullable::Value("fingerprint".to_owned()),
        sourceKey: "ignored-row-source".to_owned(),
        sourceKind: SourceKind::Forecast,
        stationSlug: "open-meteo-virtual".to_owned(),
        soilElectricalConductivityMicrosiemensPerCm: Nullable::Null,
        soilMoisturePercent: Nullable::Null,
        solarRadiationWm2: Nullable::Null,
        temperatureC: Nullable::Value(12.1),
        upstreamModel: Nullable::Value("best_match".to_owned()),
        upstreamTimezone: "GMT".to_owned(),
        uvIndex: Nullable::Value(0.2),
        validAt: JsDateOrString::String(text(valid_at)),
        waterLevelM: Nullable::Null,
        windDirectionDegrees: Nullable::Value(180.0),
        windGustMps: Nullable::Value(3.1),
        windSpeedMps: Nullable::Value(1.8),
        wetBulbGlobeTemperatureC: Nullable::Null,
    }
}

/// build one node-matching active wind row
fn active_wind_row() -> WeatherRecordRow {
    let mut row = forecast_row("12", "2026-09-09T06:00:00.000Z");
    row.adapterVersion = Nullable::Value("open-meteo-forecast-daily/v4".to_owned());
    row.contractEpoch = Nullable::Value(
        "legacy-v4/9d26d9c46dcaacc422c28e854327b11cd710625e092110786010f0687a100d83".to_owned(),
    );
    row.firstReceivedAt = JsDateOrString::String(text("2026-09-09T05:00:00.000Z"));
    row.lastReceivedAt = JsDateOrString::String(text("2026-09-09T05:00:00.000Z"));
    row.productRunAt = Nullable::Value(JsDateOrString::String(text("2026-09-08T18:00:00.000Z")));
    row.sourceConfigFingerprint = Nullable::Value(
        "ceb83ac4ba3ddc421a31043794ad450a859ecc31643506f93f64a28feb15e5b4".to_owned(),
    );
    row.sourceKey = "open-meteo-forecast-v4".to_owned();
    row
}

/// build the admitted active wind runtime
fn active_wind_runtime(evaluated_at_epoch_ms: i64) -> AdmittedWindCanaryRuntime {
    AdmittedWindCanaryRuntime {
        environment_kill_switch: None,
        authorization: WindCanaryAuthorizationWindow::TimeLimited {
            activated_at_epoch_ms: 1_788_826_944_734,
            expires_at_epoch_ms: 1_790_036_544_734,
        },
        expires_at: Some(instant(1_790_036_544_734, "2026-09-22T00:22:24.734Z")),
        enabled_metric_bands: vec![
            ForecastAdjustmentMetricBand {
                leadBand: ForecastLeadBandKey::Value001024,
                metric: ForecastAdjustmentMetric::WindGustMps,
            },
            ForecastAdjustmentMetricBand {
                leadBand: ForecastLeadBandKey::Value001024,
                metric: ForecastAdjustmentMetric::WindSpeedMps,
            },
        ],
        evaluated_at_epoch_ms,
        evidence: AdmittedWindCanaryEvidence {
            active_bundle_sha256:
                "5e8b2e3932111621af6785a1b16dfd22edc0a2d26059c6e396654c70119abbe1".to_owned(),
            authorization_sha256:
                "fc4895cb45ffb59a725ead664ae20924399585a9d2fa5f5d068f40589b523537".to_owned(),
            candidate_artifact_sha256:
                "454e7b91a34407a6ace79150dfc5799c37ced2adbdab915b710f18180c3cd488".to_owned(),
            transfer_report_sha256:
                "2c68ba8be5d5343a8d358cee7e21bb157f60c54e6c0bf2bc4b0978f50ec50582".to_owned(),
        },
    }
}

/// build one preverified core correction
fn correction(
    metric: ForecastAdjustmentMetric,
    raw_value: f64,
    coefficient: f64,
    enabled: bool,
) -> CoreAdjustmentApplicationResult {
    apply_core_adjustment(&CoreAdjustmentApplicationInput {
        calendarFingerprintMatches: true,
        coefficient: Nullable::Value(coefficient),
        enabled,
        envelope: Nullable::Value(CoreAdjustmentApplicationInputEnvelope {
            maximum: 150.0,
            minimum: 0.0,
        }),
        identityMatches: true,
        metric,
        rawForecastValue: raw_value,
        rawWindSpeedMps: Nullable::Value(1.8),
        rootAvailable: true,
    })
}

/// build one node-derived row correction output
fn active_wind_output() -> AdmittedWindRowOutput {
    AdmittedWindRowOutput::Corrections(Box::new(AdmittedWindCorrections {
        lead_band: ForecastLeadBandKey::Value001024,
        raw_forecast_provenance: ForecastAdjustmentRawForecastProvenance {
            adapterVersion: "open-meteo-forecast-daily/v4".to_owned(),
            cohort: ForecastAdjustmentRawForecastProvenanceCohortLiteral,
            contractEpoch:
                "legacy-v4/9d26d9c46dcaacc422c28e854327b11cd710625e092110786010f0687a100d83"
                    .to_owned(),
            dataset: "forecast".to_owned(),
            referenceAt: "2026-09-08T18:00:00.000Z".to_owned(),
            referenceKind: ForecastAdjustmentRawForecastProvenanceReferenceKindLiteral,
            sourceConfigFingerprint:
                "ceb83ac4ba3ddc421a31043794ad450a859ecc31643506f93f64a28feb15e5b4".to_owned(),
            sourceKey: "open-meteo-forecast-v4".to_owned(),
            targetLeadHours: 12,
            upstreamModel: "best_match".to_owned(),
            validAt: "2026-09-09T06:00:00.000Z".to_owned(),
        },
        wind_direction_degrees: Some(correction(
            ForecastAdjustmentMetric::WindDirectionDegrees,
            180.0,
            0.0,
            false,
        )),
        wind_gust_mps: Some(correction(
            ForecastAdjustmentMetric::WindGustMps,
            3.1,
            -12.0,
            true,
        )),
        wind_speed_mps: Some(correction(
            ForecastAdjustmentMetric::WindSpeedMps,
            1.8,
            -8.0,
            true,
        )),
    }))
}

#[test]
// project raw forecast rows before appending aligned decisions
fn projects_canonical_forecast_records() {
    let site = group_active_sites(vec![active_source("12"), active_source("13")])
        .data
        .remove(0);
    let time_resolver = time_resolver();
    let mut second = forecast_row("13", "2026-08-22T07:00:00.000Z");
    second.productRunAt = Nullable::Null;
    second.precipitationMm = Nullable::Null;
    let records = project_forecast_weather_records(
        vec![forecast_row("12", "2026-08-22T06:00:00.000Z"), second],
        &site,
        &instant(1_787_374_800_000, "2026-08-22T05:00:00.000Z"),
        &time_resolver,
        InactiveForecastDecisionConfig::default(),
    )
    .expect("representative forecasts must project");

    assert_eq!(records[0].id, "forecast-record-12");
    assert_eq!(records[1].id, "forecast-record-13");
    assert_eq!(records[0].validAt, "2026-08-22T06:00:00.000Z");
    assert_eq!(records[0].freshness.ageSeconds, 0);
    assert_eq!(
        records[0].productRunAt,
        Nullable::Value("2026-08-22T05:00:00.000Z".to_owned())
    );
    assert_eq!(records[1].productRunAt, Nullable::Null);
    assert_eq!(records[0].metrics.temperatureC, Nullable::Value(12.1));
    assert_eq!(records[0].metrics.blackGlobeTemperatureC, Nullable::Null);
    assert_eq!(records[0].provenance.sourceKey, "forecast-12");
    assert_eq!(records[0].provenance.providerKey, "open-meteo");
    let Nullable::Value(provider) = &records[0].metadata.provider else {
        panic!("provider metadata must be public");
    };
    let Nullable::Value(sensors) = &provider.propertySensors else {
        panic!("property sensors must be projected");
    };
    assert_eq!(sensors.len(), 1);
    assert_eq!(sensors[0].airQuality, FieldPresence::Missing);
    assert_eq!(
        sensors[0].readings.0,
        vec![("temperatureC".to_owned(), 20.0)]
    );
    assert_eq!(
        records[0].rainAdjustment.rawBestMatchPrecipitationMm,
        Nullable::Value(0.1),
    );
    assert_eq!(
        records[1].rainAdjustment.rawBestMatchPrecipitationMm,
        Nullable::Null,
    );
    assert_eq!(
        records[0].rainAdjustment.reasonCode,
        Nullable::Value("model_unavailable".to_owned()),
    );
    let ApiForecastAdjustmentDecision::Domain(ForecastAdjustmentDecision::FailRaw(adjustment)) =
        &records[0].adjustment
    else {
        panic!("inactive generic adjustment must fail raw");
    };
    assert_eq!(
        adjustment.reasonCode,
        ForecastAdjustmentReasonCode::RegistryInactive
    );
    assert_eq!(
        adjustment.state,
        ForecastAdjustmentFailRawDecisionStateEnum::Disabled
    );
    assert_eq!(
        records[0].temperatureAdjustment.reasonCode,
        Nullable::Value(TemperatureCanaryDecisionReasonValue::RegistryInactive),
    );
    assert_eq!(
        records[0].temperatureAdjustment.state,
        ForecastTemperatureCanaryDecisionV1StateEnum::Disabled,
    );
    assert_eq!(
        records[0].temperatureAdjustment.rawBestMatchTemperatureC,
        Nullable::Value(12.1),
    );
}

#[test]
// project exact startup-disabled reason codes
fn projects_configured_inactive_reasons() {
    let site = group_active_sites(vec![active_source("12")]).data.remove(0);
    let time_resolver = time_resolver();
    let records = project_forecast_weather_records(
        vec![forecast_row("12", "2026-08-22T06:00:00.000Z")],
        &site,
        &instant(1_787_374_800_000, "2026-08-22T05:00:00.000Z"),
        &time_resolver,
        InactiveForecastDecisionConfig {
            rain_reason_code: ApiRainAdjustmentRuntimeReasonCodeEnum::PolicyRaw,
            adjustment_reason_code: ForecastAdjustmentReasonCode::BundleInvalid,
            temperature_reason_code: TemperatureCanaryDecisionReasonValue::RegistryInvalid,
        },
    )
    .expect("configured inactive decisions must project");

    assert_eq!(
        records[0].rainAdjustment.reasonCode,
        Nullable::Value("policy_raw".to_owned()),
    );
    let ApiForecastAdjustmentDecision::Domain(ForecastAdjustmentDecision::FailRaw(adjustment)) =
        &records[0].adjustment
    else {
        panic!("configured generic adjustment must fail raw");
    };
    assert_eq!(
        adjustment.reasonCode,
        ForecastAdjustmentReasonCode::BundleInvalid
    );
    assert_eq!(
        records[0].temperatureAdjustment.reasonCode,
        Nullable::Value(TemperatureCanaryDecisionReasonValue::RegistryInvalid),
    );
}

#[test]
// reject inconsistent raw source metadata
fn rejects_inconsistent_source_metadata() {
    let site = group_active_sites(vec![active_source("12")]).data.remove(0);
    let time_resolver = time_resolver();
    let error = project_forecast_weather_records(
        vec![
            forecast_row("12", "2026-08-22T06:00:00.000Z"),
            forecast_row("99", "2026-08-22T07:00:00.000Z"),
        ],
        &site,
        &instant(1_787_374_800_000, "2026-08-22T05:00:00.000Z"),
        &time_resolver,
        InactiveForecastDecisionConfig::default(),
    )
    .expect_err("unknown source must fail raw projection");

    assert_eq!(error.code, "forecast_projection.source");
}

#[test]
// project actual active wind decision fields without changing raw metrics
fn projects_active_wind_corrections_and_evidence() {
    let mut source = active_source("12");
    source.source_key = "open-meteo-forecast-v4".to_owned();
    let site = group_active_sites(vec![source]).data.remove(0);
    let row = active_wind_row();
    let records = project_forecast_weather_records_with_wind_adjustments(
        vec![row.clone()],
        &site,
        &instant(1_788_930_000_000, "2026-09-09T05:00:00.000Z"),
        &time_resolver(),
        InactiveForecastDecisionConfig::default(),
        &active_wind_runtime(1_788_930_000_000),
        &[active_wind_output()],
    )
    .expect("active wind projection");

    assert_eq!(
        records[0].metrics.windDirectionDegrees,
        row.windDirectionDegrees
    );
    assert_eq!(records[0].metrics.windGustMps, row.windGustMps);
    assert_eq!(records[0].metrics.windSpeedMps, row.windSpeedMps);
    let ApiForecastAdjustmentDecision::Domain(ForecastAdjustmentDecision::WindCanaryActive(
        decision,
    )) = &records[0].adjustment
    else {
        panic!("wind decision must be active");
    };
    assert_eq!(
        decision.appliedMetrics,
        [
            ForecastAdjustmentWindCanaryMetric::WindGustMps,
            ForecastAdjustmentWindCanaryMetric::WindSpeedMps,
        ]
    );
    assert_eq!(
        decision.adjustedMetrics.0,
        [
            (ForecastAdjustmentWindCanaryMetric::WindGustMps, 0.0),
            (ForecastAdjustmentWindCanaryMetric::WindSpeedMps, 0.0),
        ]
    );
    assert_eq!(
        decision.authorizationSha256,
        "fc4895cb45ffb59a725ead664ae20924399585a9d2fa5f5d068f40589b523537"
    );
    assert_eq!(
        decision.candidateArtifactSha256,
        "454e7b91a34407a6ace79150dfc5799c37ced2adbdab915b710f18180c3cd488"
    );
    assert_eq!(decision.leadBand, ForecastLeadBandKey::Value001024);
    assert_eq!(decision.rawForecastProvenance.targetLeadHours, 12);

    let runtime = project_wind_canary_runtime(
        &instant(1_788_826_944_734, "2026-09-08T00:22:24.734Z"),
        &active_wind_runtime(1_788_930_000_000),
    )
    .expect("active runtime projection");
    assert_eq!(
        runtime.activationMode,
        Nullable::Value(ApiForecastAdjustmentRuntimeActivationModeEnum::WindCanary)
    );
    assert_eq!(runtime.state, ApiForecastAdjustmentRuntimeStateEnum::Active);
    assert_eq!(
        runtime.activeBundle,
        Nullable::Value(
            "5e8b2e3932111621af6785a1b16dfd22edc0a2d26059c6e396654c70119abbe1".to_owned()
        )
    );
    assert_eq!(
        runtime.enabledMetrics,
        [
            ForecastAdjustmentMetric::WindGustMps,
            ForecastAdjustmentMetric::WindSpeedMps,
        ]
    );
}

#[test]
// preserve activation precedence over source fail-raw output
fn prioritizes_runtime_expiry_then_source_fail_raw() {
    let mut source = active_source("12");
    source.source_key = "open-meteo-forecast-v4".to_owned();
    let site = group_active_sites(vec![source]).data.remove(0);
    let row_output = AdmittedWindRowOutput::FailRaw(ForecastAdjustmentReasonCode::IdentityMismatch);
    let active = project_forecast_weather_records_with_wind_adjustments(
        vec![active_wind_row()],
        &site,
        &instant(1_788_930_000_000, "2026-09-09T05:00:00.000Z"),
        &time_resolver(),
        InactiveForecastDecisionConfig::default(),
        &active_wind_runtime(1_788_930_000_000),
        std::slice::from_ref(&row_output),
    )
    .expect("source fail-raw projection");
    let expired = project_forecast_weather_records_with_wind_adjustments(
        vec![active_wind_row()],
        &site,
        &instant(1_788_930_000_000, "2026-09-09T05:00:00.000Z"),
        &time_resolver(),
        InactiveForecastDecisionConfig::default(),
        &active_wind_runtime(1_790_036_544_734),
        &[row_output],
    )
    .expect("expired projection");

    let ApiForecastAdjustmentDecision::Domain(ForecastAdjustmentDecision::FailRaw(active)) =
        &active[0].adjustment
    else {
        panic!("source mismatch must fail raw");
    };
    assert_eq!(
        active.reasonCode,
        ForecastAdjustmentReasonCode::IdentityMismatch
    );
    assert_eq!(
        active.state,
        ForecastAdjustmentFailRawDecisionStateEnum::NotApplicable
    );
    let ApiForecastAdjustmentDecision::Domain(ForecastAdjustmentDecision::FailRaw(expired)) =
        &expired[0].adjustment
    else {
        panic!("expired runtime must fail raw");
    };
    assert_eq!(
        expired.reasonCode,
        ForecastAdjustmentReasonCode::CanaryExpired
    );
    assert_eq!(
        expired.state,
        ForecastAdjustmentFailRawDecisionStateEnum::Disabled
    );
    let projected_runtime = project_wind_canary_runtime(
        &instant(1_788_826_944_734, "2026-09-08T00:22:24.734Z"),
        &active_wind_runtime(1_790_036_544_734),
    )
    .expect("expired runtime projection");
    assert_eq!(
        projected_runtime.state,
        ApiForecastAdjustmentRuntimeStateEnum::Disabled
    );
    assert_eq!(
        projected_runtime.reasonCode,
        Nullable::Value(ApiForecastAdjustmentRuntimeReasonCodeEnum::CanaryExpired)
    );
    assert_eq!(projected_runtime.activeBundle, Nullable::Null);
}

#[test]
// skip null raw wind metrics without inventing adjusted values
fn preserves_null_wind_metrics_in_active_rows() {
    let mut source = active_source("12");
    source.source_key = "open-meteo-forecast-v4".to_owned();
    let site = group_active_sites(vec![source]).data.remove(0);
    let mut row = active_wind_row();
    row.windGustMps = Nullable::Null;
    let AdmittedWindRowOutput::Corrections(mut corrections) = active_wind_output() else {
        panic!("fixture must contain corrections");
    };
    corrections.wind_gust_mps = None;
    let records = project_forecast_weather_records_with_wind_adjustments(
        vec![row],
        &site,
        &instant(1_788_930_000_000, "2026-09-09T05:00:00.000Z"),
        &time_resolver(),
        InactiveForecastDecisionConfig::default(),
        &active_wind_runtime(1_788_930_000_000),
        &[AdmittedWindRowOutput::Corrections(corrections)],
    )
    .expect("null wind projection");

    assert_eq!(records[0].metrics.windGustMps, Nullable::Null);
    let ApiForecastAdjustmentDecision::Domain(ForecastAdjustmentDecision::WindCanaryActive(
        decision,
    )) = &records[0].adjustment
    else {
        panic!("remaining wind metric must stay active");
    };
    assert_eq!(
        decision.appliedMetrics,
        [ForecastAdjustmentWindCanaryMetric::WindSpeedMps]
    );
    assert_eq!(
        decision.adjustedMetrics.0,
        [(ForecastAdjustmentWindCanaryMetric::WindSpeedMps, 0.0)]
    );
}
