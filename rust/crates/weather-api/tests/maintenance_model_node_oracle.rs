use std::path::PathBuf;
use std::process::Command;

use weather_adjustment::AdmittedWindRowCalendar;
use weather_api::{
    CanonicalInstant, CurrentProjectionError, CurrentTimeResolver,
    MaintenanceSourceProjectionHeader, MaintenanceWindModelCalendar,
    build_wind_maintenance_source_projection, evaluate_wind_maintenance_candidate,
    evaluate_wind_maintenance_incumbent,
};
use weather_contracts::{
    ForecastAdjustmentCandidateV2, ForecastAdjustmentCandidateV2AlgorithmContractVersionLiteral,
    ForecastAdjustmentCandidateV2ContractVersionLiteral,
    ForecastAdjustmentCandidateV2RuntimeFingerprint, ForecastAdjustmentCandidateV2SiteKeyLiteral,
    ForecastAdjustmentCandidateV2TimezoneLiteral, ForecastAdjustmentCoefficient,
    ForecastAdjustmentCoefficientDaypartEnum as Daypart, ForecastAdjustmentCoefficientLevelEnum,
    ForecastAdjustmentCoefficientSeasonEnum as Season, ForecastAdjustmentForecastIdentity,
    ForecastAdjustmentForecastIdentityCohortLiteral,
    ForecastAdjustmentForecastIdentityReferenceKindLiteral, ForecastAdjustmentMetric,
    ForecastAdjustmentMetricBand, ForecastAdjustmentTrainingEnvelope,
    ForecastAdjustmentTrainingEnvelopeMetricEnum, ForecastAdjustmentTrainingProvenance,
    ForecastAdjustmentWindMaintenanceRuntimePackage,
    ForecastAdjustmentWindMaintenanceRuntimePackageContractVersionLiteral,
    ForecastAdjustmentWindMaintenanceRuntimePackageFamilyLiteral, ForecastLeadBandKey,
    JsDateOrString, JsonValue, LoadedApiForecastAdjustmentRuntime,
    LoadedForecastAdjustmentRuntimeV1, LoadedForecastAdjustmentRuntimeV1Disabled,
    LoadedForecastAdjustmentRuntimeV1DisabledBundleNull,
    LoadedForecastAdjustmentRuntimeV1DisabledReasonCodeEnum,
    LoadedForecastAdjustmentRuntimeV1DisabledStateLiteral, Nullable, SourceKind, WeatherRecordRow,
};

const SOURCE_SHA256: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const REFERENCE_AT: &str = "2026-10-08T00:00:00.000Z";
const RECEIVED_AT: &str = "2026-10-08T00:05:00.000Z";
const ISSUED_AT: &str = "2026-10-08T00:35:00.000Z";
const BASE_EPOCH_MS: i64 = 1_791_417_600_000;

/// One exact candidate row emitted by the compiled Node replay.
struct NodeRow {
    gust: Option<f64>,
    gust_applied: bool,
    speed: f64,
    speed_applied: bool,
    valid_at: String,
}

/// Exact artifact identities and all replay rows emitted by Node.
struct NodeOracle {
    bundle_sha256: String,
    candidate_sha256: String,
    coefficient_sha256: String,
    development_sha256: String,
    export_sha256: String,
    fit_sha256: String,
    incumbent_reason: String,
    rows: Vec<NodeRow>,
}

/// Fixture time owner admitting only explicitly constructed UTC values.
struct FixtureTime(Vec<(Vec<u16>, CanonicalInstant)>);

impl CurrentTimeResolver for FixtureTime {
    /// Resolve one exact source string without implementing a date parser.
    fn resolve(&self, value: &JsDateOrString) -> Result<CanonicalInstant, CurrentProjectionError> {
        // accept only raw strings shared with the compiled Node fixture
        if let JsDateOrString::String(value) = value {
            return self
                .0
                .iter()
                .find_map(|(source, instant)| (source == value).then(|| instant.clone()))
                .ok_or_else(|| model_error("fixture time was not admitted"));
        }
        Err(model_error("fixture time was not a string"))
    }
}

/// October Los Angeles calendar evidence matching the pinned Node runtime.
struct FixtureCalendar;

impl MaintenanceWindModelCalendar for FixtureCalendar {
    /// Resolve the known October fixture's fixed PDT local daypart.
    fn calendar(&self, valid_at: &str) -> Result<AdmittedWindRowCalendar, CurrentProjectionError> {
        let utc_hour = valid_at
            .get(11..13)
            .and_then(|value| value.parse::<u8>().ok())
            .ok_or_else(|| model_error("fixture valid clock differs"))?;
        let local_hour = (utc_hour + 17) % 24;
        let daypart = match local_hour {
            0..=5 => Daypart::Night,
            6..=11 => Daypart::Morning,
            12..=17 => Daypart::Afternoon,
            _ => Daypart::Evening,
        };
        Ok(AdmittedWindRowCalendar {
            daypart,
            month: 10,
            season: Season::Autumn,
        })
    }

    /// Require the exact ICU and tzdata fingerprint used by the Node oracle.
    fn fingerprint_matches(&self, icu_version: &str, tzdata_version: &str) -> bool {
        icu_version == "78.3" && tzdata_version == "2026b"
    }
}

/// Match the complete active maintenance candidate replay to compiled Node numerics.
#[test]
fn matches_compiled_node_active_wind_maintenance_model() {
    let oracle = run_node_oracle();
    let rows = wind_rows();
    let time = fixture_time();
    let issued_at = instant(BASE_EPOCH_MS + 35 * 60_000, ISSUED_AT);
    let registration_sha256 = "a".repeat(64);
    let source = build_wind_maintenance_source_projection(
        MaintenanceSourceProjectionHeader {
            registration_sha256: &registration_sha256,
            candidate_sha256: &oracle.fit_sha256,
            source_sha256: SOURCE_SHA256,
            due_key: "capture/2026-10-08T00:35:00.000Z",
            issued_at: &issued_at,
        },
        &rows,
        &time,
    )
    .expect("build the complete wind source");
    let bundle = maintenance_bundle(&oracle);

    let actual = evaluate_wind_maintenance_candidate(&source, &bundle, &FixtureCalendar)
        .expect("evaluate the admitted maintenance candidate");

    assert_eq!(actual.len(), oracle.rows.len());
    // compare every numerical result and the disabled gust band to original Node
    for (actual, expected) in actual.iter().zip(&oracle.rows) {
        assert_eq!(actual.validAt, expected.valid_at);
        assert_eq!(actual.candidateSpeedMps, expected.speed);
        assert_eq!(actual.candidateGustMps, nullable(expected.gust));
        assert_eq!(actual.speedWouldApply, expected.speed_applied);
        assert_eq!(actual.gustWouldApply, expected.gust_applied);
    }
    let incumbent = LoadedApiForecastAdjustmentRuntime::Standard(
        LoadedForecastAdjustmentRuntimeV1::Disabled(LoadedForecastAdjustmentRuntimeV1Disabled {
            bundle: LoadedForecastAdjustmentRuntimeV1DisabledBundleNull,
            reasonCode: LoadedForecastAdjustmentRuntimeV1DisabledReasonCodeEnum::BundleMissing,
            state: LoadedForecastAdjustmentRuntimeV1DisabledStateLiteral,
        }),
    );
    let replay = evaluate_wind_maintenance_incumbent(&source, &incumbent, &time, &FixtureCalendar)
        .expect("evaluate the original raw incumbent");
    assert_eq!(replay.len(), oracle.rows.len());
    // compare the saved disabled incumbent with Node's raw and unavailable-gust witnesses
    for (index, row) in replay.iter().enumerate() {
        assert_eq!(row.validAt, oracle.rows[index].valid_at);
        assert_eq!(
            row.reasonCode,
            Nullable::Value(oracle.incumbent_reason.clone())
        );
        assert_eq!(row.incumbentSpeedMps, 5.0);
        assert_eq!(
            row.incumbentGustMps,
            nullable((!(48..72).contains(&index)).then_some(8.0))
        );
        assert!(!row.speedApplied);
        assert!(!row.gustApplied);
    }
}

/// Run the direct compiled Node model replay and decode its bounded text output.
fn run_node_oracle() -> NodeOracle {
    let script =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/maintenance-model-node-oracle.mjs");
    let output = Command::new("node")
        .arg(script)
        .output()
        .expect("run maintenance model Node oracle");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = String::from_utf8(output.stdout).expect("Node emits UTF-8 fixture output");
    let mut candidate_sha256 = None;
    let mut coefficient_sha256 = None;
    let mut development_sha256 = None;
    let mut export_sha256 = None;
    let mut bundle_sha256 = None;
    let mut fit_sha256 = None;
    let mut incumbent_reason = None;
    let mut rows = Vec::with_capacity(168);
    // preserve exact source row order while decoding only the tiny fixture protocol
    for line in output.lines() {
        if let Some(value) = line.strip_prefix("candidate=") {
            candidate_sha256 = Some(value.to_owned());
        } else if let Some(value) = line.strip_prefix("coefficient=") {
            coefficient_sha256 = Some(value.to_owned());
        } else if let Some(value) = line.strip_prefix("development=") {
            development_sha256 = Some(value.to_owned());
        } else if let Some(value) = line.strip_prefix("export=") {
            export_sha256 = Some(value.to_owned());
        } else if let Some(value) = line.strip_prefix("bundle=") {
            bundle_sha256 = Some(value.to_owned());
        } else if let Some(value) = line.strip_prefix("fit=") {
            fit_sha256 = Some(value.to_owned());
        } else if let Some(value) = line.strip_prefix("incumbent=") {
            incumbent_reason = Some(value.to_owned());
        } else if line.starts_with("row|") {
            rows.push(parse_node_row(line));
        } else {
            panic!("unexpected Node oracle output");
        }
    }
    assert_eq!(rows.len(), 168);
    NodeOracle {
        bundle_sha256: bundle_sha256.expect("bundle digest"),
        candidate_sha256: candidate_sha256.expect("candidate digest"),
        coefficient_sha256: coefficient_sha256.expect("coefficient digest"),
        development_sha256: development_sha256.expect("development digest"),
        export_sha256: export_sha256.expect("export digest"),
        fit_sha256: fit_sha256.expect("fit digest"),
        incumbent_reason: incumbent_reason.expect("incumbent reason"),
        rows,
    }
}

/// Parse one exact `row|index|clock|speed|gust|speed-applied|gust-applied` witness.
fn parse_node_row(value: &str) -> NodeRow {
    let fields = value.split('|').collect::<Vec<_>>();
    assert_eq!(fields.len(), 7);
    let expected_index = fields[1].parse::<usize>().expect("row index");
    let gust = (fields[4] != "null").then(|| fields[4].parse::<f64>().expect("gust value"));
    let row = NodeRow {
        gust,
        gust_applied: parse_bool(fields[6]),
        speed: fields[3].parse().expect("speed value"),
        speed_applied: parse_bool(fields[5]),
        valid_at: fields[2].to_owned(),
    };
    assert_eq!(
        row.valid_at,
        valid_at(u64::try_from(expected_index + 1).expect("lead"))
    );
    row
}

/// Parse one exact JavaScript boolean spelling.
fn parse_bool(value: &str) -> bool {
    match value {
        "true" => true,
        "false" => false,
        _ => panic!("invalid Node boolean"),
    }
}

/// Build the generated maintenance package around the exact Node candidate tables.
fn maintenance_bundle(oracle: &NodeOracle) -> ForecastAdjustmentWindMaintenanceRuntimePackage {
    let candidate = candidate(oracle);
    ForecastAdjustmentWindMaintenanceRuntimePackage {
        bundleSha256: oracle.bundle_sha256.clone(),
        candidateSha256: oracle.fit_sha256.clone(),
        source: candidate.forecastIdentity.clone(),
        candidate,
        contractVersion: ForecastAdjustmentWindMaintenanceRuntimePackageContractVersionLiteral,
        dueMonth: "2026-10".to_owned(),
        family: ForecastAdjustmentWindMaintenanceRuntimePackageFamilyLiteral,
    }
}

/// Build the exact selected root-coefficient candidate used by Node.
fn candidate(oracle: &NodeOracle) -> ForecastAdjustmentCandidateV2 {
    let pairs = metric_bands();
    ForecastAdjustmentCandidateV2 {
        algorithmContractVersion: ForecastAdjustmentCandidateV2AlgorithmContractVersionLiteral,
        candidateArtifactSha256: oracle.candidate_sha256.clone(),
        coefficientPayloadSha256: oracle.coefficient_sha256.clone(),
        coefficients: pairs.iter().map(root_coefficient).collect(),
        contractVersion: ForecastAdjustmentCandidateV2ContractVersionLiteral,
        developmentReportSha256: oracle.development_sha256.clone(),
        enabledMetricBands: pairs.clone(),
        evaluationEpochId: "maintenance-2026-10".to_owned(),
        exportManifestSha256: oracle.export_sha256.clone(),
        finalTrainingCutoff: "2026-09-01T06:59:59.999Z".to_owned(),
        forecastIdentity: forecast_identity(),
        metricPolicies: Vec::new(),
        runtimeFingerprint: ForecastAdjustmentCandidateV2RuntimeFingerprint {
            icuVersion: "78.3".to_owned(),
            tzdataVersion: "2026b".to_owned(),
        },
        siteKey: ForecastAdjustmentCandidateV2SiteKeyLiteral,
        timezone: ForecastAdjustmentCandidateV2TimezoneLiteral,
        trainingEnvelopes: pairs.iter().map(training_envelope).collect(),
        trainingProvenance: ForecastAdjustmentTrainingProvenance {
            aggregationContractSha256: "1".repeat(64),
            coordinateManifestSha256: "2".repeat(64),
            metricEligibilitySha256: "3".repeat(64),
            observationSourceLineageSha256: "4".repeat(64),
            observationStationManifestSha256: "5".repeat(64),
            spatialWeightSha256: "6".repeat(64),
        },
    }
}

/// Return all thirteen source-ordered supported maintenance metric bands.
fn metric_bands() -> Vec<ForecastAdjustmentMetricBand> {
    let bands = [
        ForecastLeadBandKey::Value001024,
        ForecastLeadBandKey::Value025048,
        ForecastLeadBandKey::Value049072,
        ForecastLeadBandKey::Value073096,
        ForecastLeadBandKey::Value097120,
        ForecastLeadBandKey::Value121144,
        ForecastLeadBandKey::Value145168,
    ];
    let mut pairs = bands
        .iter()
        .copied()
        .map(|lead_band| ForecastAdjustmentMetricBand {
            leadBand: lead_band,
            metric: ForecastAdjustmentMetric::WindSpeedMps,
        })
        .collect::<Vec<_>>();
    pairs.extend(
        bands
            .iter()
            .copied()
            .filter(|band| *band != ForecastLeadBandKey::Value049072)
            .map(|lead_band| ForecastAdjustmentMetricBand {
                leadBand: lead_band,
                metric: ForecastAdjustmentMetric::WindGustMps,
            }),
    );
    pairs
}

/// Build one root hierarchy coefficient from the selected Node fit.
fn root_coefficient(pair: &ForecastAdjustmentMetricBand) -> ForecastAdjustmentCoefficient {
    ForecastAdjustmentCoefficient {
        coefficient: 0.25,
        daypart: Nullable::Null,
        effectiveEventCount: 200.0,
        leadBand: pair.leadBand,
        level: ForecastAdjustmentCoefficientLevelEnum::Value1,
        metric: pair.metric,
        month: Nullable::Null,
        season: Nullable::Null,
    }
}

/// Build one selected training envelope for the source metric-band pair.
fn training_envelope(pair: &ForecastAdjustmentMetricBand) -> ForecastAdjustmentTrainingEnvelope {
    let metric = match pair.metric {
        ForecastAdjustmentMetric::WindGustMps => {
            ForecastAdjustmentTrainingEnvelopeMetricEnum::WindGustMps
        }
        ForecastAdjustmentMetric::WindSpeedMps => {
            ForecastAdjustmentTrainingEnvelopeMetricEnum::WindSpeedMps
        }
        _ => unreachable!("fixture contains only wind speed and gust"),
    };
    ForecastAdjustmentTrainingEnvelope {
        leadBand: pair.leadBand,
        maximum: 40.0,
        metric,
        minimum: 0.0,
    }
}

/// Build the exact canonical source identity used by the maintenance fit.
fn forecast_identity() -> ForecastAdjustmentForecastIdentity {
    ForecastAdjustmentForecastIdentity {
        adapterVersion: "open-meteo-forecast-daily/v4".to_owned(),
        cohort: ForecastAdjustmentForecastIdentityCohortLiteral,
        contractEpoch: "legacy-v4/9d26d9c46dcaacc422c28e854327b11cd710625e092110786010f0687a100d83"
            .to_owned(),
        dataset: "forecast".to_owned(),
        referenceKind: ForecastAdjustmentForecastIdentityReferenceKindLiteral,
        sourceConfigFingerprint: "ceb83ac4ba3ddc421a31043794ad450a859ecc31643506f93f64a28feb15e5b4"
            .to_owned(),
        sourceKey: "open-meteo-forecast-v4".to_owned(),
        upstreamModel: "best_match".to_owned(),
    }
}

/// Build all 168 exact source rows required by the maintenance evaluator.
fn wind_rows() -> Vec<WeatherRecordRow> {
    (1..=168).map(wind_row).collect()
}

/// Build one canonical Best Match source row for a selected lead.
fn wind_row(lead_hours: u64) -> WeatherRecordRow {
    let identity = forecast_identity();
    WeatherRecordRow {
        adapterVersion: Nullable::Value(identity.adapterVersion),
        apparentTemperatureC: Nullable::Null,
        blackGlobeTemperatureC: Nullable::Null,
        cloudCoverPercent: Nullable::Null,
        contentHash: format!("{lead_hours:064x}"),
        contractEpoch: Nullable::Value(identity.contractEpoch),
        deviceModel: Nullable::Null,
        deviceSerial: Nullable::Null,
        deviceVendor: Nullable::Null,
        firstReceivedAt: wire_time(RECEIVED_AT),
        id: format!("model-row-{lead_hours}"),
        lastReceivedAt: wire_time(RECEIVED_AT),
        pm25MicrogramsPerCubicMeter: Nullable::Null,
        precipitationMm: Nullable::Null,
        precipitationRateMmPerHour: Nullable::Null,
        pressureHpa: Nullable::Null,
        productRunAt: Nullable::Value(wire_time(REFERENCE_AT)),
        providerKey: "open-meteo".to_owned(),
        providerMetadata: Nullable::Value(vec![entry(
            "dataset",
            JsonValue::String(text(&identity.dataset)),
        )]),
        qualityMetadata: Nullable::Null,
        relativeHumidityPercent: Nullable::Null,
        revisionCount: lead_hours - 1,
        siteSlug: "ballydidean".to_owned(),
        sourceConfigFingerprint: Nullable::Value(identity.sourceConfigFingerprint),
        sourceId: "12".to_owned(),
        sourceKey: identity.sourceKey,
        sourceKind: SourceKind::Forecast,
        stationSlug: "open-meteo-virtual".to_owned(),
        soilElectricalConductivityMicrosiemensPerCm: Nullable::Null,
        soilMoisturePercent: Nullable::Null,
        solarRadiationWm2: Nullable::Null,
        temperatureC: Nullable::Null,
        upstreamModel: Nullable::Value(identity.upstreamModel),
        upstreamTimezone: "GMT".to_owned(),
        uvIndex: Nullable::Null,
        validAt: wire_time(&valid_at(lead_hours)),
        waterLevelM: Nullable::Null,
        wetBulbGlobeTemperatureC: Nullable::Null,
        windDirectionDegrees: Nullable::Null,
        windGustMps: if (49..=72).contains(&lead_hours) {
            Nullable::Null
        } else {
            Nullable::Value(8.0)
        },
        windSpeedMps: Nullable::Value(5.0),
    }
}

/// Build the exact admitted UTC table used by source projection.
fn fixture_time() -> FixtureTime {
    let mut values = vec![
        admitted_time(REFERENCE_AT, BASE_EPOCH_MS),
        admitted_time(RECEIVED_AT, BASE_EPOCH_MS + 5 * 60_000),
        admitted_time(ISSUED_AT, BASE_EPOCH_MS + 35 * 60_000),
    ];
    // admit every prospective hourly source clock explicitly
    for lead_hours in 1..=168 {
        values.push(admitted_time(
            &valid_at(lead_hours),
            BASE_EPOCH_MS + i64::try_from(lead_hours).expect("lead") * 3_600_000,
        ));
    }
    FixtureTime(values)
}

/// Build one fixture time mapping.
fn admitted_time(value: &str, epoch_milliseconds: i64) -> (Vec<u16>, CanonicalInstant) {
    (text(value), instant(epoch_milliseconds, value))
}

/// Build one canonical instant.
fn instant(epoch_milliseconds: i64, utc: &str) -> CanonicalInstant {
    CanonicalInstant {
        epoch_milliseconds,
        utc: utc.to_owned(),
    }
}

/// Build one exact October hourly UTC string.
fn valid_at(lead_hours: u64) -> String {
    let day = 8 + lead_hours / 24;
    let hour = lead_hours % 24;
    format!("2026-10-{day:02}T{hour:02}:00:00.000Z")
}

/// Build one generated UTF-16 string.
fn text(value: &str) -> Vec<u16> {
    value.encode_utf16().collect()
}

/// Build one generated timestamp string.
fn wire_time(value: &str) -> JsDateOrString {
    JsDateOrString::String(text(value))
}

/// Build one ordered generated JSON entry.
fn entry(key: &str, value: JsonValue) -> (Vec<u16>, JsonValue) {
    (text(key), value)
}

/// Preserve Node nullability in the generated output contract.
fn nullable(value: Option<f64>) -> Nullable<f64> {
    value.map_or(Nullable::Null, Nullable::Value)
}

/// Build one local fixture error without exposing model internals.
fn model_error(detail: &str) -> CurrentProjectionError {
    CurrentProjectionError {
        code: "maintenance_model.fixture",
        detail: detail.to_owned(),
    }
}
