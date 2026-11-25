//! Compiled-Node parity for canonical wind maintenance candidate material.

use std::{
    cell::RefCell,
    collections::BTreeMap,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};

use weather_contracts::{
    DevelopmentLosoFoldResultV1, DevelopmentLosoFoldResultV1ContractVersionLiteral,
    DevelopmentLosoFoldResultV1FoldEnum as Fold,
    ForecastAdjustmentCandidateV2RuntimeFingerprint as RuntimeFingerprint,
    ForecastAdjustmentCoefficient, ForecastAdjustmentCoefficientLevelEnum as Level,
    ForecastAdjustmentForecastIdentity,
    ForecastAdjustmentForecastIdentityCohortLiteral as ForecastCohort,
    ForecastAdjustmentForecastIdentityReferenceKindLiteral as ReferenceKind,
    ForecastAdjustmentInsufficientDataReportV1, ForecastAdjustmentMetric as Metric,
    ForecastAdjustmentMetricBand, ForecastAdjustmentTrainingEnvelope,
    ForecastAdjustmentTrainingEnvelopeMetricEnum as EnvelopeMetric,
    ForecastAdjustmentTrainingProvenance, ForecastLeadBandKey as LeadBand,
    ForecastObservationProviderFamily as Provider, ForecastObservationStation,
    ForecastObservationStationKey as StationKey, JsonValue, Nullable,
};
use weather_database::Sha256Digest;
use weather_runtime::{
    ApiMaintenanceReceiptCanonicalValues, ApiMaintenanceShadowEvidenceValues,
    MaintenanceReceiptScalar, PreparedWindMaintenanceCandidate, WindMaintenanceCandidateIdentity,
    encode_wind_maintenance_fit_publication, prepare_wind_maintenance_candidate,
    wind_maintenance_auxiliary_model_sha256,
};
use weather_worker::wind_maintenance_development::{
    WindAuxiliaryModel, WindDevelopmentQualification, WindDevelopmentQualifiedPair,
};

const SNAPSHOT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const FINAL_CUTOFF: &str = "2026-07-25T06:59:59.999Z";
const DUE_MONTH: &str = "2026-09";
const LEADS: [LeadBand; 7] = [
    LeadBand::Value001024,
    LeadBand::Value025048,
    LeadBand::Value049072,
    LeadBand::Value073096,
    LeadBand::Value097120,
    LeadBand::Value121144,
    LeadBand::Value145168,
];
const SCOREABLE_STATIONS: [StationKey; 5] = [
    StationKey::AmbientMaxweather,
    StationKey::BallydideanEcowitt,
    StationKey::NetatmoNearby,
    StationKey::Tempest126537,
    StationKey::Tempest64255,
];

#[derive(Debug)]
struct Preimage {
    bytes: Vec<u8>,
    sha256: String,
}

#[derive(Debug)]
struct Oracle {
    bytes: BTreeMap<String, Vec<u8>>,
    cells: BTreeMap<String, Vec<u8>>,
    preimages: BTreeMap<String, Preimage>,
    projection: Vec<String>,
}

struct Values<'a> {
    oracle: &'a Oracle,
    trace: RefCell<Vec<String>>,
}

impl ApiMaintenanceReceiptCanonicalValues for Values<'_> {
    /// Admit only the exact fixture cutoff instant.
    fn canonical_utc_epoch_milliseconds(&self, value: &str) -> Option<i64> {
        (value == FINAL_CUTOFF).then_some(0)
    }

    /// Return only original Node SHA-256 results for exact captured preimages.
    fn sha256(&self, bytes: &[u8]) -> Option<Sha256Digest> {
        self.oracle
            .preimages
            .values()
            .find(|preimage| preimage.bytes == bytes)
            .map(|preimage| Sha256Digest::admitted(&preimage.sha256))
    }

    /// Candidate material never creates maintenance HTTP receipts.
    fn unsigned_receipt_sha256(
        &self,
        _: &[(&str, MaintenanceReceiptScalar<'_>)],
    ) -> Option<Sha256Digest> {
        unreachable!("candidate preparation does not encode an HTTP receipt")
    }
}

impl ApiMaintenanceShadowEvidenceValues for Values<'_> {
    /// This fixture supplies typed material rather than parsed source JSON.
    fn decode_shadow_json(&self, _: &[u8]) -> Option<JsonValue> {
        unreachable!("candidate preparation receives typed material")
    }

    /// Candidate preparation uses only the recursively sorted canonical owner.
    fn encode_shadow_ordered_json(&self, _: &JsonValue) -> Option<Vec<u8>> {
        unreachable!("candidate preparation does not use ordered source encoding")
    }

    /// Candidate preparation contains no embedded comparator payload.
    fn decode_shadow_comparator_base64(&self, _: &[u16]) -> Option<Vec<u8>> {
        unreachable!()
    }

    /// Canonicalize the actual projected value and compare with the independently authored fixture.
    fn encode_shadow_sorted_json(&self, doc: &JsonValue) -> Option<Vec<u8>> {
        let label = document_label(doc)?;
        self.trace.borrow_mut().push(label.clone());
        let expected = if let Some(key) = label.strip_prefix("cell:") {
            self.oracle.cells.get(key)?
        } else if let Some(key) = label.strip_suffix("-bytes") {
            self.oracle.bytes.get(key)?
        } else {
            &self.oracle.preimages.get(&label)?.bytes
        };
        let mut transport = String::new();
        fixture_frame(&mut transport, doc);
        let mut child = Command::new("node")
            .arg(candidate_oracle_path())
            .arg("--encode-values")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("fixture canonicalizer starts");
        child
            .stdin
            .take()
            .expect("fixture stdin")
            .write_all(transport.as_bytes())
            .expect("send exact typed values");
        let result = child.wait_with_output().expect("canonicalizer returns");
        assert!(result.status.success(), "canonicalize {label}");
        assert!(result.stdout == *expected, "preimage differs for {label}");
        Some(result.stdout)
    }

    /// This fixture supplies no decoded maintenance package.
    fn decode_shadow_maintenance_package(
        &self,
        _: &JsonValue,
    ) -> Option<weather_contracts::ForecastAdjustmentMaintenanceRuntimePackage> {
        unreachable!()
    }

    /// This fixture supplies no portable rain model.
    fn decode_shadow_rain_artifact(
        &self,
        _: &JsonValue,
    ) -> Option<weather_contracts::RainHurdleWindPortableArtifact> {
        unreachable!()
    }

    /// Candidate preparation never examines a legacy runtime string.
    fn check_legacy_wind_runtime_value(&self, _: &[u16]) -> Option<()> {
        unreachable!()
    }
}

/// Borrow the compiled-Node fixture for both expected documents and actual value encoding.
fn candidate_oracle_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/wind_maintenance_candidate_node_oracle.mjs")
}

/// Carry generated values over a fixture-only line frame, retaining bit patterns and code units.
fn fixture_frame(output: &mut String, value: &JsonValue) {
    use std::fmt::Write as _;
    match value {
        JsonValue::Null => output.push_str("z\n"),
        JsonValue::Boolean(true) => output.push_str("t\n"),
        JsonValue::Boolean(false) => output.push_str("f\n"),
        JsonValue::Number(number) => writeln!(output, "n{:016x}", number.to_bits()).unwrap(),
        JsonValue::String(units) => fixture_string_frame(output, units),
        JsonValue::Array(items) => {
            writeln!(output, "a{}", items.len()).unwrap();
            // keep array order unchanged through fixture transport
            for item in items {
                fixture_frame(output, item);
            }
        }
        JsonValue::Object(fields) => {
            writeln!(output, "o{}", fields.len()).unwrap();
            // let original Node code own sorting and escaping
            for (key, item) in fields {
                fixture_string_frame(output, key);
                fixture_frame(output, item);
            }
        }
    }
}

/// Preserve exact UTF-16 units in one fixture frame.
fn fixture_string_frame(output: &mut String, units: &[u16]) {
    use std::fmt::Write as _;
    output.push('s');
    // carry string units without interpolation or replacement
    for unit in units {
        write!(output, "{unit:04x}").unwrap();
    }
    output.push('\n');
}

/// Compare exact preimages, hashes, typed artifacts, and both CLI wrapper documents.
#[test]
fn matches_compiled_wind_maintenance_candidate_material() {
    let oracle = load_oracle();
    let values = Values {
        oracle: &oracle,
        trace: RefCell::new(Vec::new()),
    };
    let pair = canonical_pairs().remove(0);
    let auxiliary_coefficient = coefficient(&pair, 0);
    let auxiliary_sha = wind_maintenance_auxiliary_model_sha256(
        &values,
        WindAuxiliaryModel {
            coefficients: &[auxiliary_coefficient],
            fold: Fold::Value1,
            metric_band: &pair,
            physical_station_key: StationKey::AmbientMaxweather,
        },
    )
    .expect("auxiliary model preimage is canonical");
    assert_eq!(auxiliary_sha.as_str(), oracle.preimages["auxiliary"].sha256);

    let snapshot = Sha256Digest::admitted(SNAPSHOT);
    let forecast_identity = forecast_identity();
    let runtime_fingerprint = RuntimeFingerprint {
        icuVersion: "fixture-icu".to_owned(),
        tzdataVersion: "fixture-tz".to_owned(),
    };
    let training_provenance = training_provenance();
    let identity = WindMaintenanceCandidateIdentity {
        due_month: DUE_MONTH,
        d0: "2025-06-26",
        d401: "2026-08-01",
        snapshot_manifest_sha256: &snapshot,
        final_training_cutoff: FINAL_CUTOFF,
        forecast_identity: &forecast_identity,
        runtime_fingerprint: &runtime_fingerprint,
        training_provenance: &training_provenance,
    };
    let stations = stations();
    let sufficient =
        prepare_wind_maintenance_candidate(&values, &identity, &stations, qualification(13))
            .expect("all thirteen pairs prepare a development candidate");
    let sufficient_publication =
        encode_wind_maintenance_fit_publication(&values, DUE_MONTH, &sufficient)
            .expect("development publication is canonical");
    let PreparedWindMaintenanceCandidate::DevelopmentCandidate(artifacts) = &sufficient else {
        panic!("thirteen pairs must retain a candidate")
    };
    assert_eq!(artifacts.candidate_bytes, oracle.bytes["candidate"]);
    assert_eq!(
        artifacts.development_report_bytes,
        oracle.bytes["development"]
    );
    assert_eq!(
        sufficient_publication,
        oracle.bytes["development-publication"]
    );

    let insufficient =
        prepare_wind_maintenance_candidate(&values, &identity, &stations, qualification(12))
            .expect("an incomplete mask produces sanitized insufficiency");
    let insufficient_publication =
        encode_wind_maintenance_fit_publication(&values, DUE_MONTH, &insufficient)
            .expect("insufficiency publication is canonical");
    let PreparedWindMaintenanceCandidate::Insufficient { report, bytes } = &insufficient else {
        panic!("twelve pairs must not retain a candidate")
    };
    assert_eq!(bytes, &oracle.bytes["insufficient"]);
    assert_eq!(
        insufficient_publication,
        oracle.bytes["insufficient-publication"]
    );
    assert_eq!(actual_projection(artifacts, report), oracle.projection);
    assert_encode_trace(&values.trace.borrow());
}

/// Load the independent compiled-Node preimages and human-readable field projection.
fn load_oracle() -> Oracle {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/wind_maintenance_candidate_node_oracle.mjs");
    let output = Command::new("node")
        .arg(path)
        .output()
        .expect("compiled Node wind-candidate oracle starts");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("oracle output is UTF-8");
    let mut oracle = Oracle {
        bytes: BTreeMap::new(),
        cells: BTreeMap::new(),
        preimages: BTreeMap::new(),
        projection: Vec::new(),
    };
    // retain the oracle's exact row order while indexing binary materials by label
    for line in stdout.lines() {
        let fields = line.split('\t').collect::<Vec<_>>();
        match fields.as_slice() {
            ["PREIMAGE", label, bytes, sha256] => {
                oracle.preimages.insert(
                    (*label).to_owned(),
                    Preimage {
                        bytes: decode_hex(bytes),
                        sha256: (*sha256).to_owned(),
                    },
                );
            }
            ["BYTES", label, bytes] => {
                oracle.bytes.insert((*label).to_owned(), decode_hex(bytes));
            }
            ["CELL", coefficient_bits, bytes] => {
                oracle
                    .cells
                    .insert((*coefficient_bits).to_owned(), decode_hex(bytes));
            }
            [label, ..]
                if matches!(
                    *label,
                    "SUMMARY"
                        | "PAIR"
                        | "COEFFICIENT"
                        | "ENVELOPE"
                        | "FOLD"
                        | "POLICY"
                        | "IDENTITY"
                        | "PROVENANCE"
                        | "RUNTIME"
                ) =>
            {
                oracle.projection.push(line.to_owned());
            }
            _ => panic!("unexpected oracle row: {line}"),
        }
    }
    oracle
}

/// Decode bounded oracle hexadecimal without adopting a production JSON or hash codec.
fn decode_hex(value: &str) -> Vec<u8> {
    assert_eq!(value.len() % 2, 0);
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|digits| {
            let text = std::str::from_utf8(digits).expect("hex digits are ASCII");
            u8::from_str_radix(text, 16).expect("oracle emits lowercase hex")
        })
        .collect()
}

/// Classify only the closed canonical fixture documents by their authored fields.
fn document_label(value: &JsonValue) -> Option<String> {
    match value {
        JsonValue::Object(_) if field(value, "confirmationOpened").is_some() => {
            let state = json_text(field(value, "state")?)?;
            Some(format!(
                "{}-publication-bytes",
                if state == "development_candidate" {
                    "development"
                } else {
                    "insufficient"
                }
            ))
        }
        JsonValue::Object(_) if field(value, "physicalStationKey").is_some() => {
            Some("auxiliary".to_owned())
        }
        JsonValue::Object(_) if field(value, "adapterVersion").is_some() => {
            Some("forecast-identity".to_owned())
        }
        JsonValue::Object(_) if field(value, "aggregationContractSha256").is_some() => {
            Some("training-provenance".to_owned())
        }
        JsonValue::Object(_) if field(value, "generatedBeforeHoldout").is_some() => Some(
            if field(value, "developmentReportSha256").is_some() {
                "development-bytes"
            } else {
                "development"
            }
            .to_owned(),
        ),
        JsonValue::Object(_) if field(value, "algorithmContractVersion").is_some() => Some(
            if field(value, "candidateArtifactSha256").is_some() {
                "candidate-bytes"
            } else {
                "candidate"
            }
            .to_owned(),
        ),
        JsonValue::Object(_) if field(value, "failedGates").is_some() => Some(
            if field(value, "reportSha256").is_some() {
                "insufficient-bytes"
            } else {
                "insufficient"
            }
            .to_owned(),
        ),
        JsonValue::Object(_) if field(value, "d0").is_some() => Some("epoch".to_owned()),
        JsonValue::Object(_) if field(value, "effectiveEventCount").is_some() => {
            let JsonValue::Number(coefficient) = field(value, "coefficient")? else {
                return None;
            };
            Some(format!("cell:{}", bits(*coefficient)))
        }
        JsonValue::Array(values)
            if values
                .first()
                .is_some_and(|value| field(value, "correctionMaximum").is_some()) =>
        {
            Some("metric-policies".to_owned())
        }
        JsonValue::Array(values)
            if values
                .first()
                .is_some_and(|value| field(value, "effectiveEventCount").is_some()) =>
        {
            Some("coefficients".to_owned())
        }
        _ => None,
    }
}

/// Borrow one object field by exact UTF-16 code units.
fn field<'a>(value: &'a JsonValue, key: &str) -> Option<&'a JsonValue> {
    let JsonValue::Object(fields) = value else {
        return None;
    };
    fields
        .iter()
        .find_map(|(name, value)| name.iter().copied().eq(key.encode_utf16()).then_some(value))
}

/// Decode one fixture JSON string without replacement.
fn json_text(value: &JsonValue) -> Option<String> {
    let JsonValue::String(value) = value else {
        return None;
    };
    String::from_utf16(value).ok()
}

/// Construct all thirteen original gust and speed pairs in canonical order.
fn canonical_pairs() -> Vec<ForecastAdjustmentMetricBand> {
    LEADS
        .iter()
        .copied()
        .filter(|lead| *lead != LeadBand::Value049072)
        .map(|lead_band| ForecastAdjustmentMetricBand {
            leadBand: lead_band,
            metric: Metric::WindGustMps,
        })
        .chain(
            LEADS
                .iter()
                .copied()
                .map(|lead_band| ForecastAdjustmentMetricBand {
                    leadBand: lead_band,
                    metric: Metric::WindSpeedMps,
                }),
        )
        .collect()
}

/// Construct a reversed qualification to make production establish canonical pair order.
fn qualification(pair_count: usize) -> WindDevelopmentQualification {
    let mut pairs = canonical_pairs()
        .into_iter()
        .take(pair_count)
        .enumerate()
        .map(|(index, pair)| WindDevelopmentQualifiedPair {
            folds: std::array::from_fn(|fold| passing_fold(&pair, index, fold + 1)),
            coefficients: vec![coefficient(&pair, index)],
            envelope: envelope(&pair, index),
            pair,
        })
        .collect::<Vec<_>>();
    pairs.reverse();
    WindDevelopmentQualification::Qualified(pairs)
}

/// Construct one passing fold with all five original provider-bound identities.
fn passing_fold(
    pair: &ForecastAdjustmentMetricBand,
    pair_index: usize,
    fold: usize,
) -> DevelopmentLosoFoldResultV1 {
    DevelopmentLosoFoldResultV1 {
        auxiliaryModelSha256s: (0..SCOREABLE_STATIONS.len())
            .map(|station| format!("{:064x}", pair_index * 25 + fold * 5 + station + 1))
            .collect(),
        bootstrapLowerBound: 0.01,
        contractVersion: DevelopmentLosoFoldResultV1ContractVersionLiteral,
        fold: fold_value(fold),
        materialHarmSliceKeys: Vec::new(),
        metricBand: pair.clone(),
        nonnegativeStationFraction: 1.0,
        passed: true,
        providerBalancedAdjustedLoss: 9.0,
        providerBalancedRawLoss: 10.0,
        providerBalancedSkill: 0.1,
        providerFamilies: vec![
            Provider::Ambient,
            Provider::Ecowitt,
            Provider::Netatmo,
            Provider::Tempest,
        ],
        scoreableStationKeys: SCOREABLE_STATIONS.to_vec(),
    }
}

/// Construct one deterministic root coefficient for a qualified pair.
fn coefficient(pair: &ForecastAdjustmentMetricBand, index: usize) -> ForecastAdjustmentCoefficient {
    ForecastAdjustmentCoefficient {
        coefficient: index as f64 + 0.25,
        daypart: Nullable::Null,
        effectiveEventCount: 200.0 + index as f64,
        leadBand: pair.leadBand,
        level: Level::Value1,
        metric: pair.metric,
        month: Nullable::Null,
        season: Nullable::Null,
    }
}

/// Construct one deterministic scalar final-training envelope.
fn envelope(
    pair: &ForecastAdjustmentMetricBand,
    index: usize,
) -> ForecastAdjustmentTrainingEnvelope {
    ForecastAdjustmentTrainingEnvelope {
        leadBand: pair.leadBand,
        maximum: 30.0 + index as f64,
        metric: match pair.metric {
            Metric::WindGustMps => EnvelopeMetric::WindGustMps,
            Metric::WindSpeedMps => EnvelopeMetric::WindSpeedMps,
            _ => unreachable!("fixture contains only scalar wind metrics"),
        },
        minimum: 1.0 + index as f64,
    }
}

/// Construct the exact original served forecast identity.
fn forecast_identity() -> ForecastAdjustmentForecastIdentity {
    ForecastAdjustmentForecastIdentity {
        adapterVersion: "open-meteo-forecast-daily/v4".to_owned(),
        cohort: ForecastCohort,
        contractEpoch: "legacy-v4/9d26d9c46dcaacc422c28e854327b11cd710625e092110786010f0687a100d83"
            .to_owned(),
        dataset: "forecast".to_owned(),
        referenceKind: ReferenceKind,
        sourceConfigFingerprint: "ceb83ac4ba3ddc421a31043794ad450a859ecc31643506f93f64a28feb15e5b4"
            .to_owned(),
        sourceKey: "open-meteo-forecast-v4".to_owned(),
        upstreamModel: "best_match".to_owned(),
    }
}

/// Construct the exact original observation training provenance.
fn training_provenance() -> ForecastAdjustmentTrainingProvenance {
    ForecastAdjustmentTrainingProvenance {
        aggregationContractSha256:
            "9c309ef5a00780167570746ad6c31b9128c266db50954fe4645287e1f2b31e64".to_owned(),
        coordinateManifestSha256:
            "04bfd93a03c393e977c8767a9aca6fe2a4cba9c263cb46e6987fa733b666ba58".to_owned(),
        metricEligibilitySha256: "53731954b347836a26500b05a195ca15cf26214c4d561fe482c5ff87ef56a82e"
            .to_owned(),
        observationSourceLineageSha256:
            "261a134589a12c1bbbd9a783343950317fd1fbc87e08383e60e805b7761566cc".to_owned(),
        observationStationManifestSha256:
            "a1f76440c056987bbb434d5315e4916f961deeb2951fe889d785943f559cdd49".to_owned(),
        spatialWeightSha256: "8ed5ce70d33edd4a5166049d9938cbaaf800151b6a0b3345d3005419e9041c74"
            .to_owned(),
    }
}

/// Construct the five station catalog identities used by every synthetic fold.
fn stations() -> Vec<ForecastObservationStation> {
    [
        (StationKey::AmbientMaxweather, Provider::Ambient),
        (StationKey::BallydideanEcowitt, Provider::Ecowitt),
        (StationKey::NetatmoNearby, Provider::Netatmo),
        (StationKey::Tempest126537, Provider::Tempest),
        (StationKey::Tempest64255, Provider::Tempest),
    ]
    .into_iter()
    .map(|(key, provider_family)| ForecastObservationStation {
        acceptedSourceKeys: Vec::new(),
        distanceMeters: 0.0,
        eligibleMetrics: vec![Metric::WindGustMps, Metric::WindSpeedMps],
        key,
        latitude: 0.0,
        longitude: 0.0,
        nearestRank: 1,
        providerFamily: provider_family,
        unnormalizedSpatialWeight: 1.0,
    })
    .collect()
}

/// Project all generated artifact fields covered by the independent Node oracle.
fn actual_projection(
    artifacts: &weather_runtime::WindMaintenanceCandidateArtifacts,
    insufficient: &ForecastAdjustmentInsufficientDataReportV1,
) -> Vec<String> {
    let candidate = &artifacts.candidate;
    let development = &artifacts.development_report;
    let mut rows = vec![
        [
            "SUMMARY".to_owned(),
            candidate.candidateArtifactSha256.clone(),
            candidate.coefficientPayloadSha256.clone(),
            candidate.developmentReportSha256.clone(),
            candidate.evaluationEpochId.clone(),
            development.developmentReportSha256.clone(),
            candidate.enabledMetricBands.len().to_string(),
            candidate.coefficients.len().to_string(),
            candidate.trainingEnvelopes.len().to_string(),
            development.folds.len().to_string(),
            insufficient.reportSha256.clone(),
        ]
        .join("\t"),
    ];
    rows.extend(
        candidate
            .enabledMetricBands
            .iter()
            .enumerate()
            .map(|(index, pair)| {
                format!(
                    "PAIR\t{index}\t{}\t{}",
                    metric_name(pair.metric),
                    lead_name(pair.leadBand)
                )
            }),
    );
    rows.extend(
        candidate
            .coefficients
            .iter()
            .enumerate()
            .map(|(index, item)| coefficient_row(index, item)),
    );
    rows.extend(
        candidate
            .trainingEnvelopes
            .iter()
            .enumerate()
            .map(|(index, item)| envelope_row(index, item)),
    );
    rows.extend(
        development
            .folds
            .iter()
            .enumerate()
            .map(|(index, item)| fold_row(index, item)),
    );
    rows.extend(
        candidate
            .metricPolicies
            .iter()
            .enumerate()
            .map(|(index, item)| {
                [
                    "POLICY".to_owned(),
                    index.to_string(),
                    metric_name(item.metric).to_owned(),
                    bits(item.correctionMinimum),
                    bits(item.correctionMaximum),
                    bits(item.finalMinimum),
                    bits(item.finalMaximum),
                    item.finalMaximumExclusive.to_string(),
                    item.wrapsFinalValue.to_string(),
                ]
                .join("\t")
            }),
    );
    rows.push(
        [
            "IDENTITY",
            &candidate.forecastIdentity.adapterVersion,
            "legacy_v4_retrieval_snapshot",
            &candidate.forecastIdentity.contractEpoch,
            &candidate.forecastIdentity.dataset,
            "retrieval_snapshot",
            &candidate.forecastIdentity.sourceConfigFingerprint,
            &candidate.forecastIdentity.sourceKey,
            &candidate.forecastIdentity.upstreamModel,
        ]
        .join("\t"),
    );
    rows.push(
        [
            "PROVENANCE",
            &candidate.trainingProvenance.aggregationContractSha256,
            &candidate.trainingProvenance.coordinateManifestSha256,
            &candidate.trainingProvenance.metricEligibilitySha256,
            &candidate.trainingProvenance.observationSourceLineageSha256,
            &candidate
                .trainingProvenance
                .observationStationManifestSha256,
            &candidate.trainingProvenance.spatialWeightSha256,
        ]
        .join("\t"),
    );
    rows.push(
        [
            "RUNTIME",
            &candidate.runtimeFingerprint.icuVersion,
            &candidate.runtimeFingerprint.tzdataVersion,
            &candidate.finalTrainingCutoff,
            &candidate.exportManifestSha256,
        ]
        .join("\t"),
    );
    rows
}

/// Project one generated coefficient with exact binary64 values.
fn coefficient_row(index: usize, item: &ForecastAdjustmentCoefficient) -> String {
    [
        "COEFFICIENT".to_owned(),
        index.to_string(),
        metric_name(item.metric).to_owned(),
        lead_name(item.leadBand).to_owned(),
        level_name(item.level).to_owned(),
        "<null>".to_owned(),
        "<null>".to_owned(),
        "<null>".to_owned(),
        bits(item.coefficient),
        bits(item.effectiveEventCount),
    ]
    .join("\t")
}

/// Project one generated envelope with exact binary64 values.
fn envelope_row(index: usize, item: &ForecastAdjustmentTrainingEnvelope) -> String {
    [
        "ENVELOPE".to_owned(),
        index.to_string(),
        envelope_metric_name(item.metric).to_owned(),
        lead_name(item.leadBand).to_owned(),
        bits(item.minimum),
        bits(item.maximum),
    ]
    .join("\t")
}

/// Project one generated development fold and its retained evidence order.
fn fold_row(index: usize, item: &DevelopmentLosoFoldResultV1) -> String {
    [
        "FOLD".to_owned(),
        index.to_string(),
        metric_name(item.metricBand.metric).to_owned(),
        lead_name(item.metricBand.leadBand).to_owned(),
        fold_name(item.fold).to_owned(),
        item.passed.to_string(),
        bits(item.bootstrapLowerBound),
        bits(item.providerBalancedRawLoss),
        bits(item.providerBalancedAdjustedLoss),
        bits(item.providerBalancedSkill),
        bits(item.nonnegativeStationFraction),
        item.auxiliaryModelSha256s.join(","),
        item.providerFamilies
            .iter()
            .map(|&family| provider_name(family))
            .collect::<Vec<_>>()
            .join(","),
        item.scoreableStationKeys
            .iter()
            .map(|&key| station_name(key))
            .collect::<Vec<_>>()
            .join(","),
    ]
    .join("\t")
}

/// Verify the runtime requested every expected preimage class and exact wrapper once.
fn assert_encode_trace(trace: &[String]) {
    for required in [
        "auxiliary",
        "forecast-identity",
        "training-provenance",
        "metric-policies",
        "development",
        "development-bytes",
        "coefficients",
        "epoch",
        "candidate",
        "candidate-bytes",
        "insufficient",
        "insufficient-bytes",
        "development-publication-bytes",
        "insufficient-publication-bytes",
    ] {
        assert!(
            trace.iter().any(|item| item == required),
            "missing {required}"
        );
    }
    assert_eq!(
        trace
            .iter()
            .filter(|item| item.starts_with("cell:"))
            .count(),
        13
    );
}

/// Encode one binary64 result as big-endian hexadecimal bits.
fn bits(value: f64) -> String {
    format!("{:016x}", value.to_bits())
}

/// Map one numeric fold fixture index to the generated enum.
const fn fold_value(value: usize) -> Fold {
    match value {
        1 => Fold::Value1,
        2 => Fold::Value2,
        3 => Fold::Value3,
        4 => Fold::Value4,
        5 => Fold::Value5,
        _ => unreachable!(),
    }
}

/// Render one generated fold wire value.
const fn fold_name(value: Fold) -> &'static str {
    match value {
        Fold::Value1 => "1",
        Fold::Value2 => "2",
        Fold::Value3 => "3",
        Fold::Value4 => "4",
        Fold::Value5 => "5",
    }
}

/// Render one generated coefficient level wire value.
const fn level_name(value: Level) -> &'static str {
    match value {
        Level::Value1 => "1",
        Level::Value2 => "2",
        Level::Value3 => "3",
    }
}

/// Render one generated metric wire value.
const fn metric_name(value: Metric) -> &'static str {
    match value {
        Metric::RelativeHumidityPercent => "relativeHumidityPercent",
        Metric::TemperatureC => "temperatureC",
        Metric::WindDirectionDegrees => "windDirectionDegrees",
        Metric::WindGustMps => "windGustMps",
        Metric::WindSpeedMps => "windSpeedMps",
    }
}

/// Render one generated envelope metric wire value.
const fn envelope_metric_name(value: EnvelopeMetric) -> &'static str {
    match value {
        EnvelopeMetric::RelativeHumidityPercent => "relativeHumidityPercent",
        EnvelopeMetric::TemperatureC => "temperatureC",
        EnvelopeMetric::WindGustMps => "windGustMps",
        EnvelopeMetric::WindSpeedMps => "windSpeedMps",
    }
}

/// Render one generated lead-band wire value.
const fn lead_name(value: LeadBand) -> &'static str {
    match value {
        LeadBand::Value001024 => "001-024",
        LeadBand::Value025048 => "025-048",
        LeadBand::Value049072 => "049-072",
        LeadBand::Value073096 => "073-096",
        LeadBand::Value097120 => "097-120",
        LeadBand::Value121144 => "121-144",
        LeadBand::Value145168 => "145-168",
    }
}

/// Render one generated provider-family wire value.
const fn provider_name(value: Provider) -> &'static str {
    match value {
        Provider::Ambient => "ambient",
        Provider::Ecowitt => "ecowitt",
        Provider::Netatmo => "netatmo",
        Provider::Tempest => "tempest",
    }
}

/// Render one generated station-key wire value.
const fn station_name(value: StationKey) -> &'static str {
    match value {
        StationKey::AmbientMaxweather => "ambient-maxweather",
        StationKey::AmbientMerlin => "ambient-merlin",
        StationKey::BallydideanEcowitt => "ballydidean-ecowitt",
        StationKey::NetatmoNearby => "netatmo-nearby",
        StationKey::Tempest126537 => "tempest-126537",
        StationKey::Tempest168853 => "tempest-168853",
        StationKey::Tempest201058 => "tempest-201058",
        StationKey::Tempest203055 => "tempest-203055",
        StationKey::Tempest225947 => "tempest-225947",
        StationKey::Tempest38270 => "tempest-38270",
        StationKey::Tempest64255 => "tempest-64255",
    }
}
