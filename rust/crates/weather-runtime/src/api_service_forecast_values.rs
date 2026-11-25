//! concrete forecast and health response values in the node producer's member order

use weather_contracts as c;
use weather_contracts::{FieldPresence, JsonValue};

use crate::api_service_json_values::{
    array, boolean, enum_text, member, nullable, number, object, text, unsigned,
};
use crate::api_service_weather_values::{site_value, weather_record_members, weather_record_value};

/// project one forecast response without changing the source producer's member order.
pub(crate) fn forecast_value(value: &c::ApiForecastResponse) -> JsonValue {
    object([
        (
            "adjustmentRuntime",
            adjustment_runtime_value(&value.adjustmentRuntime),
        ),
        (
            "airQualityContext",
            array(&value.airQualityContext, forecast_record_value),
        ),
        (
            "rainAdjustmentRuntime",
            rain_runtime_value(&value.rainAdjustmentRuntime),
        ),
        ("data", array(&value.data, forecast_record_value)),
        ("days", forecast_days_value(value.days)),
        ("generatedAt", text(&value.generatedAt)),
        (
            "pressureContext",
            array(&value.pressureContext, weather_record_value),
        ),
        ("site", site_value(&value.site)),
        (
            "temperatureAdjustmentRuntime",
            temperature_runtime_value(&value.temperatureAdjustmentRuntime),
        ),
    ])
}

/// project one health response with the exact nested response shape.
pub(crate) fn health_value(value: &c::ApiHealthResponse) -> JsonValue {
    let data = &value.data;
    object([(
        "data",
        object([
            (
                "adjustmentRuntime",
                adjustment_runtime_value(&data.adjustmentRuntime),
            ),
            (
                "temperatureAdjustmentRuntime",
                temperature_runtime_value(&data.temperatureAdjustmentRuntime),
            ),
            (
                "database",
                enum_text(
                    data.database,
                    c::ApiHealthResponseDataDatabaseEnum::WIRE_VALUES,
                ),
            ),
            (
                "live",
                boolean(c::ApiHealthResponseDataLiveLiteral::WIRE_VALUE),
            ),
            ("migration", health_migration_value(&data.migration)),
            ("ready", boolean(data.ready)),
            ("version", text(&data.version)),
            ("worker", health_worker_value(&data.worker)),
        ]),
    )])
}

/// preserve the forecast-day numeric literal rather than converting it to text.
fn forecast_days_value(value: c::ForecastDays) -> JsonValue {
    // keep the generated closed numeric union exhaustive
    match value {
        c::ForecastDays::Value1 => unsigned(1),
        c::ForecastDays::Value5 => unsigned(5),
        c::ForecastDays::Value10 => unsigned(10),
    }
}

/// append forecast-only decisions after the spread base weather record.
fn forecast_record_value(value: &c::ApiForecastWeatherRecord) -> JsonValue {
    let mut fields = weather_record_members(
        &value.freshness,
        &value.id,
        &value.metadata,
        &value.metrics,
        &value.productRunAt,
        &value.provenance,
        &value.receivedAt,
        value.revisionCount,
        &value.validAt,
    );
    fields.push(member(
        "rainAdjustment",
        rain_decision_value(&value.rainAdjustment),
    ));
    fields.push(member("adjustment", adjustment_value(&value.adjustment)));
    fields.push(member(
        "temperatureAdjustment",
        temperature_decision_value(&value.temperatureAdjustment),
    ));
    JsonValue::Object(fields)
}

/// retain optional maintenance fields before the ordinary wind runtime fields.
fn adjustment_runtime_value(value: &c::ApiForecastAdjustmentRuntime) -> JsonValue {
    let mut fields = Vec::with_capacity(15);
    push_present_text(&mut fields, "actionSha256", &value.actionSha256);
    push_present_text(
        &mut fields,
        "fullMemberRootSha256",
        &value.fullMemberRootSha256,
    );
    push_present_text(&mut fields, "policyReportSha256", &value.policyReportSha256);
    fields.extend([
        member(
            "activationMode",
            nullable(&value.activationMode, |item| {
                enum_text(
                    *item,
                    c::ApiForecastAdjustmentRuntimeActivationModeEnum::WIRE_VALUES,
                )
            }),
        ),
        member(
            "activeBundle",
            nullable(&value.activeBundle, |item| text(item)),
        ),
        member(
            "authorizationSha256",
            nullable(&value.authorizationSha256, |item| text(item)),
        ),
        member(
            "candidateArtifactSha256",
            nullable(&value.candidateArtifactSha256, |item| text(item)),
        ),
        member(
            "enabledMetrics",
            array(&value.enabledMetrics, |item| {
                enum_text(*item, c::ForecastAdjustmentMetric::WIRE_VALUES)
            }),
        ),
        member(
            "evaluationReportSha256",
            nullable(&value.evaluationReportSha256, |item| text(item)),
        ),
        member("expiresAt", nullable(&value.expiresAt, |item| text(item))),
        member("loadedAt", text(&value.loadedAt)),
        member(
            "qualificationReceiptSha256",
            nullable(&value.qualificationReceiptSha256, |item| text(item)),
        ),
        member(
            "reasonCode",
            nullable(&value.reasonCode, |item| {
                enum_text(
                    *item,
                    c::ApiForecastAdjustmentRuntimeReasonCodeEnum::WIRE_VALUES,
                )
            }),
        ),
        member(
            "state",
            enum_text(
                value.state,
                c::ApiForecastAdjustmentRuntimeStateEnum::WIRE_VALUES,
            ),
        ),
        member(
            "transferReportSha256",
            nullable(&value.transferReportSha256, |item| text(item)),
        ),
    ]);
    JsonValue::Object(fields)
}

/// omit a missing optional source property and retain a present string verbatim.
fn push_present_text(
    fields: &mut Vec<(Vec<u16>, JsonValue)>,
    key: &str,
    value: &FieldPresence<String>,
) {
    // distinguish a missing member from every present string
    if let FieldPresence::Value(value) = value {
        fields.push(member(key, text(value)));
    }
}

/// project the independent temperature runtime descriptor.
fn temperature_runtime_value(value: &c::ApiForecastTemperatureAdjustmentRuntime) -> JsonValue {
    object([
        (
            "actionSha256",
            nullable(&value.actionSha256, |item| text(item)),
        ),
        (
            "activationMode",
            nullable(&value.activationMode, |item| {
                enum_text(
                    *item,
                    c::ApiForecastTemperatureAdjustmentRuntimeActivationModeEnum::WIRE_VALUES,
                )
            }),
        ),
        (
            "activeBundle",
            nullable(&value.activeBundle, |item| text(item)),
        ),
        (
            "authorizationSha256",
            nullable(&value.authorizationSha256, |item| text(item)),
        ),
        ("expiresAt", nullable(&value.expiresAt, |item| text(item))),
        (
            "fullMemberRootSha256",
            nullable(&value.fullMemberRootSha256, |item| text(item)),
        ),
        ("loadedAt", text(&value.loadedAt)),
        (
            "policyReportSha256",
            nullable(&value.policyReportSha256, |item| text(item)),
        ),
        (
            "reasonCode",
            nullable(&value.reasonCode, |item| {
                enum_text(
                    *item,
                    c::ApiForecastTemperatureAdjustmentRuntimeReasonCodeEnum::WIRE_VALUES,
                )
            }),
        ),
        (
            "source",
            nullable(&value.source, temperature_runtime_source_value),
        ),
        (
            "state",
            enum_text(
                value.state,
                c::ApiForecastTemperatureAdjustmentRuntimeStateEnum::WIRE_VALUES,
            ),
        ),
    ])
}

/// project the bounded temperature collector status.
fn temperature_runtime_source_value(
    value: &c::ApiForecastTemperatureAdjustmentRuntimeSource,
) -> JsonValue {
    object([
        ("adaptiveReady", boolean(value.adaptiveReady)),
        ("firstReceivedAt", text(&value.firstReceivedAt)),
        ("hourCount", unsigned(value.hourCount)),
        (
            "latestRunInitializedAt",
            text(&value.latestRunInitializedAt),
        ),
        (
            "stateReason",
            nullable(&value.stateReason, |item| text(item)),
        ),
        (
            "stateStatus",
            enum_text(
                value.stateStatus,
                c::ApiForecastTemperatureAdjustmentRuntimeSourceStateStatusEnum::WIRE_VALUES,
            ),
        ),
    ])
}

/// project the rain runtime descriptor without exposing private inputs.
fn rain_runtime_value(value: &c::ApiRainAdjustmentRuntime) -> JsonValue {
    object([
        (
            "state",
            enum_text(
                value.state,
                c::ApiRainAdjustmentRuntimeStateEnum::WIRE_VALUES,
            ),
        ),
        (
            "activeBundle",
            nullable(&value.activeBundle, |item| text(item)),
        ),
        (
            "reasonCode",
            nullable(&value.reasonCode, |item| {
                enum_text(
                    *item,
                    c::ApiRainAdjustmentRuntimeReasonCodeEnum::WIRE_VALUES,
                )
            }),
        ),
        ("loadedAt", text(&value.loadedAt)),
        ("source", nullable(&value.source, rain_runtime_source_value)),
    ])
}

/// project one active rain runtime source summary.
fn rain_runtime_source_value(value: &c::ApiRainAdjustmentRuntimeSource) -> JsonValue {
    object([
        ("runInitializedAt", text(&value.runInitializedAt)),
        ("firstReceivedAt", text(&value.firstReceivedAt)),
        ("decisionAt", text(&value.decisionAt)),
        ("hourCount", unsigned(value.hourCount)),
    ])
}

/// project one rain decision attached to a forecast hour.
fn rain_decision_value(value: &c::ApiRainAdjustmentDecision) -> JsonValue {
    object([
        (
            "contractVersion",
            text(c::ApiRainAdjustmentDecisionContractVersionLiteral::WIRE_VALUE),
        ),
        (
            "state",
            enum_text(
                value.state,
                c::ApiRainAdjustmentDecisionStateEnum::WIRE_VALUES,
            ),
        ),
        ("reasonCode", nullable(&value.reasonCode, |item| text(item))),
        (
            "bundleSha256",
            nullable(&value.bundleSha256, |item| text(item)),
        ),
        (
            "correctedPrecipitationMm",
            nullable(&value.correctedPrecipitationMm, |item| number(*item)),
        ),
        (
            "rawBestMatchPrecipitationMm",
            nullable(&value.rawBestMatchPrecipitationMm, |item| number(*item)),
        ),
        (
            "sourceForecast",
            nullable(&value.sourceForecast, rain_source_value),
        ),
    ])
}

/// project one rain decision source hour.
fn rain_source_value(value: &c::ApiRainAdjustmentDecisionSourceForecast) -> JsonValue {
    object([
        ("runInitializedAt", text(&value.runInitializedAt)),
        ("firstReceivedAt", text(&value.firstReceivedAt)),
        ("validAt", text(&value.validAt)),
        ("rawPrecipitationMm", number(value.rawPrecipitationMm)),
        ("modelLeadHours", unsigned(value.modelLeadHours)),
        ("decisionAt", text(&value.decisionAt)),
        (
            "upstreamModel",
            text(c::ApiRainAdjustmentDecisionSourceForecastUpstreamModelLiteral::WIRE_VALUE),
        ),
        (
            "providerKey",
            text(c::ApiRainAdjustmentDecisionSourceForecastProviderKeyLiteral::WIRE_VALUE),
        ),
    ])
}

/// project either supported wind decision family without adding a discriminator.
fn adjustment_value(value: &c::ApiForecastAdjustmentDecision) -> JsonValue {
    // preserve the structural union's original object shape
    match value {
        c::ApiForecastAdjustmentDecision::Domain(value) => domain_adjustment_value(value),
        c::ApiForecastAdjustmentDecision::MaintenanceQualified(value) => {
            maintenance_adjustment_value(value)
        }
    }
}

/// project one domain wind adjustment union member.
fn domain_adjustment_value(value: &c::ForecastAdjustmentDecision) -> JsonValue {
    // retain each producer's distinct member order
    match value {
        c::ForecastAdjustmentDecision::Active(value) => active_adjustment_value(value),
        c::ForecastAdjustmentDecision::WindCanaryActive(value) => {
            wind_canary_adjustment_value(value)
        }
        c::ForecastAdjustmentDecision::FailRaw(value) => fail_raw_adjustment_value(value),
    }
}

/// project the schema-complete unchanged-raw decision.
fn fail_raw_adjustment_value(value: &c::ForecastAdjustmentFailRawDecision) -> JsonValue {
    object([
        ("adjustedMetrics", JsonValue::Object(Vec::new())),
        ("appliedMetrics", JsonValue::Array(Vec::new())),
        (
            "contractVersion",
            text(c::ForecastAdjustmentFailRawDecisionContractVersionLiteral::WIRE_VALUE),
        ),
        (
            "reasonCode",
            enum_text(
                value.reasonCode,
                c::ForecastAdjustmentReasonCode::WIRE_VALUES,
            ),
        ),
        (
            "state",
            enum_text(
                value.state,
                c::ForecastAdjustmentFailRawDecisionStateEnum::WIRE_VALUES,
            ),
        ),
    ])
}

/// project one qualified standard adjustment decision.
fn active_adjustment_value(value: &c::ForecastAdjustmentActiveDecision) -> JsonValue {
    object([
        (
            "adjustedMetrics",
            adjusted_metrics_value(&value.adjustedMetrics.0),
        ),
        (
            "algorithmContractVersion",
            text(c::ForecastAdjustmentActiveDecisionAlgorithmContractVersionLiteral::WIRE_VALUE),
        ),
        (
            "appliedMetrics",
            array(&value.appliedMetrics, |item| {
                enum_text(*item, c::ForecastAdjustmentMetric::WIRE_VALUES)
            }),
        ),
        (
            "candidateArtifactSha256",
            text(&value.candidateArtifactSha256),
        ),
        (
            "contractVersion",
            text(c::ForecastAdjustmentActiveDecisionContractVersionLiteral::WIRE_VALUE),
        ),
        (
            "evaluationReportSha256",
            text(&value.evaluationReportSha256),
        ),
        (
            "leadBand",
            enum_text(value.leadBand, c::ForecastLeadBandKey::WIRE_VALUES),
        ),
        (
            "qualificationReceiptSha256",
            text(&value.qualificationReceiptSha256),
        ),
        (
            "rawForecastProvenance",
            raw_forecast_provenance_value(&value.rawForecastProvenance),
        ),
        ("reasonCode", JsonValue::Null),
        (
            "state",
            text(c::ForecastAdjustmentActiveDecisionStateLiteral::WIRE_VALUE),
        ),
    ])
}

/// project one active transfer-canary wind decision.
fn wind_canary_adjustment_value(
    value: &c::ForecastAdjustmentWindCanaryActiveDecision,
) -> JsonValue {
    object([
        (
            "activationKind",
            text(c::ForecastAdjustmentWindCanaryActiveDecisionActivationKindLiteral::WIRE_VALUE),
        ),
        ("adjustedMetrics", wind_adjusted_metrics_value(&value.adjustedMetrics.0)),
        (
            "algorithmContractVersion",
            text(
                c::ForecastAdjustmentWindCanaryActiveDecisionAlgorithmContractVersionLiteral::WIRE_VALUE,
            ),
        ),
        (
            "appliedMetrics",
            array(&value.appliedMetrics, |item| {
                enum_text(*item, c::ForecastAdjustmentWindCanaryMetric::WIRE_VALUES)
            }),
        ),
        ("authorizationSha256", text(&value.authorizationSha256)),
        ("candidateArtifactSha256", text(&value.candidateArtifactSha256)),
        (
            "contractVersion",
            text(c::ForecastAdjustmentWindCanaryActiveDecisionContractVersionLiteral::WIRE_VALUE),
        ),
        ("leadBand", enum_text(value.leadBand, c::ForecastLeadBandKey::WIRE_VALUES)),
        (
            "rawForecastProvenance",
            raw_forecast_provenance_value(&value.rawForecastProvenance),
        ),
        ("reasonCode", JsonValue::Null),
        (
            "state",
            text(c::ForecastAdjustmentWindCanaryActiveDecisionStateLiteral::WIRE_VALUE),
        ),
        ("transferReportSha256", text(&value.transferReportSha256)),
    ])
}

/// project one maintenance-qualified wind decision.
fn maintenance_adjustment_value(
    value: &c::ForecastAdjustmentWindQualifiedMaintenanceDecision,
) -> JsonValue {
    object([
        ("actionSha256", text(&value.actionSha256)),
        (
            "activationKind",
            text(
                c::ForecastAdjustmentWindQualifiedMaintenanceDecisionActivationKindLiteral::WIRE_VALUE,
            ),
        ),
        ("adjustedMetrics", adjusted_metrics_value(&value.adjustedMetrics.0)),
        (
            "algorithmContractVersion",
            text(
                c::ForecastAdjustmentWindQualifiedMaintenanceDecisionAlgorithmContractVersionLiteral::WIRE_VALUE,
            ),
        ),
        (
            "appliedMetrics",
            array(&value.appliedMetrics, |item| {
                enum_text(*item, c::ForecastAdjustmentWindCanaryMetric::WIRE_VALUES)
            }),
        ),
        ("candidateArtifactSha256", text(&value.candidateArtifactSha256)),
        (
            "contractVersion",
            text(
                c::ForecastAdjustmentWindQualifiedMaintenanceDecisionContractVersionLiteral::WIRE_VALUE,
            ),
        ),
        ("fullMemberRootSha256", text(&value.fullMemberRootSha256)),
        ("leadBand", enum_text(value.leadBand, c::ForecastLeadBandKey::WIRE_VALUES)),
        ("policyReportSha256", text(&value.policyReportSha256)),
        (
            "rawForecastProvenance",
            raw_forecast_provenance_value(&value.rawForecastProvenance),
        ),
        ("reasonCode", JsonValue::Null),
        (
            "state",
            text(c::ForecastAdjustmentWindQualifiedMaintenanceDecisionStateLiteral::WIRE_VALUE),
        ),
    ])
}

/// preserve the insertion order of one partial standard metric record.
fn adjusted_metrics_value(values: &[(c::ForecastAdjustmentMetric, f64)]) -> JsonValue {
    JsonValue::Object(
        values
            .iter()
            .map(|(metric, value)| {
                member(
                    metric_wire_name(*metric, c::ForecastAdjustmentMetric::WIRE_VALUES),
                    number(*value),
                )
            })
            .collect(),
    )
}

/// preserve the insertion order of one partial wind-only metric record.
fn wind_adjusted_metrics_value(
    values: &[(c::ForecastAdjustmentWindCanaryMetric, f64)],
) -> JsonValue {
    JsonValue::Object(
        values
            .iter()
            .map(|(metric, value)| {
                member(
                    metric_wire_name(*metric, c::ForecastAdjustmentWindCanaryMetric::WIRE_VALUES),
                    number(*value),
                )
            })
            .collect(),
    )
}

/// resolve one generated metric enum to its exact object-key spelling.
fn metric_wire_name<T: Copy + Eq>(value: T, wire: &'static [(&'static str, T)]) -> &'static str {
    wire.iter()
        .find_map(|(name, candidate)| (*candidate == value).then_some(*name))
        .expect("generated metric value has a wire spelling")
}

/// project the immutable raw forecast identity used by every wind decision.
fn raw_forecast_provenance_value(value: &c::ForecastAdjustmentRawForecastProvenance) -> JsonValue {
    object([
        ("adapterVersion", text(&value.adapterVersion)),
        (
            "cohort",
            text(c::ForecastAdjustmentRawForecastProvenanceCohortLiteral::WIRE_VALUE),
        ),
        ("contractEpoch", text(&value.contractEpoch)),
        ("dataset", text(&value.dataset)),
        ("referenceAt", text(&value.referenceAt)),
        (
            "referenceKind",
            text(c::ForecastAdjustmentRawForecastProvenanceReferenceKindLiteral::WIRE_VALUE),
        ),
        (
            "sourceConfigFingerprint",
            text(&value.sourceConfigFingerprint),
        ),
        ("sourceKey", text(&value.sourceKey)),
        ("targetLeadHours", unsigned(value.targetLeadHours)),
        ("upstreamModel", text(&value.upstreamModel)),
        ("validAt", text(&value.validAt)),
    ])
}

/// project one temperature decision attached to a forecast hour.
fn temperature_decision_value(value: &c::ForecastTemperatureCanaryDecisionV1) -> JsonValue {
    object([
        (
            "branch",
            nullable(&value.branch, |item| {
                enum_text(
                    *item,
                    c::ForecastTemperatureCanaryDecisionV1BranchEnum::WIRE_VALUES,
                )
            }),
        ),
        (
            "bundleSha256",
            nullable(&value.bundleSha256, |item| text(item)),
        ),
        (
            "contractVersion",
            text(c::ForecastTemperatureCanaryDecisionV1ContractVersionLiteral::WIRE_VALUE),
        ),
        (
            "correctedTemperatureC",
            nullable(&value.correctedTemperatureC, |item| number(*item)),
        ),
        (
            "rawBestMatchTemperatureC",
            nullable(&value.rawBestMatchTemperatureC, |item| number(*item)),
        ),
        (
            "reasonCode",
            nullable(&value.reasonCode, |item| {
                enum_text(*item, c::TemperatureCanaryDecisionReasonValue::WIRE_VALUES)
            }),
        ),
        (
            "recentErrorStateSha256",
            nullable(&value.recentErrorStateSha256, |item| text(item)),
        ),
        (
            "sourceForecast",
            nullable(&value.sourceForecast, temperature_source_value),
        ),
        (
            "state",
            enum_text(
                value.state,
                c::ForecastTemperatureCanaryDecisionV1StateEnum::WIRE_VALUES,
            ),
        ),
    ])
}

/// project one temperature source hour with its appended operational horizon.
fn temperature_source_value(
    value: &c::ForecastTemperatureCanaryDecisionV1SourceForecast,
) -> JsonValue {
    object([
        ("adapterVersion", text(&value.adapterVersion)),
        (
            "dataset",
            text(c::TemperatureCanarySourceForecastDatasetLiteral::WIRE_VALUE),
        ),
        ("firstReceivedAt", text(&value.firstReceivedAt)),
        (
            "modelCycle",
            enum_text(
                value.modelCycle,
                c::TemperatureCanarySourceForecastModelCycleEnum::WIRE_VALUES,
            ),
        ),
        ("modelLeadHours", unsigned(value.modelLeadHours)),
        (
            "providerKey",
            text(c::TemperatureCanarySourceForecastProviderKeyLiteral::WIRE_VALUE),
        ),
        (
            "providerResponseSha256",
            text(&value.providerResponseSha256),
        ),
        (
            "rawRelativeHumidityPercent",
            nullable(&value.rawRelativeHumidityPercent, |item| number(*item)),
        ),
        ("rawTemperatureC", number(value.rawTemperatureC)),
        (
            "rawWindSpeedMps",
            nullable(&value.rawWindSpeedMps, |item| number(*item)),
        ),
        ("runInitializedAt", text(&value.runInitializedAt)),
        (
            "upstreamModel",
            text(c::TemperatureCanarySourceForecastUpstreamModelLiteral::WIRE_VALUE),
        ),
        ("validAt", text(&value.validAt)),
        (
            "operationalHorizonHours",
            unsigned(value.operationalHorizonHours),
        ),
    ])
}

/// project one health migration descriptor.
fn health_migration_value(value: &c::ApiHealthResponseDataMigration) -> JsonValue {
    object([
        (
            "status",
            enum_text(
                value.status,
                c::ApiHealthResponseDataMigrationStatusEnum::WIRE_VALUES,
            ),
        ),
        ("version", nullable(&value.version, |item| text(item))),
    ])
}

/// project one coarse worker freshness descriptor.
fn health_worker_value(value: &c::ApiHealthResponseDataWorker) -> JsonValue {
    object([(
        "freshness",
        enum_text(
            value.freshness,
            c::ApiHealthResponseDataWorkerFreshnessEnum::WIRE_VALUES,
        ),
    )])
}
