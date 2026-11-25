//! In-memory parity for archive wind-fit input admission and ordinary insufficiency.

use std::{cell::RefCell, collections::BTreeMap, path::PathBuf, process::Command};

use weather_contracts::{
    ForecastAdjustmentCandidateV2RuntimeFingerprint as RuntimeFingerprint,
    ForecastAdjustmentCoefficientDaypartEnum as Daypart,
    ForecastAdjustmentCoefficientSeasonEnum as Season, JsonValue,
};
use weather_database::Sha256Digest;
use weather_runtime::{
    ApiMaintenanceReceiptCanonicalValues, ApiMaintenanceShadowEvidenceValues,
    MaintenanceReceiptScalar, WindFitInputCalendar, WindFitInputError, WindFitLocalFeatures,
    WindMaintenanceCliError, WindMaintenanceCliIo, WindMaintenanceCutoffError,
    WindMaintenanceFileMetadata, WindMaintenanceLocalCalendar, run_wind_maintenance_cli,
    run_wind_maintenance_document, wind_maintenance_process_exit_code,
};
use weather_worker::wind_maintenance_development::AdmittedWindRowCalendar;

const DUE_MONTH: &str = "2026-10";
const VALID_AT: &str = "2026-01-01T12:00:00.000Z";
const OPENED_AT: &str = "2026-08-15T12:00:00.000Z";
const FIXTURE_INPUT: &[u8] = b"admitted-wind-fixture";

#[derive(Debug)]
struct Preimage {
    bytes: Vec<u8>,
    sha256: String,
}

#[derive(Debug)]
struct ExpectedResult {
    contract_version: String,
    state: String,
    failed_gate: String,
    report_sha256: String,
    snapshot_sha256: String,
    total_row_count: usize,
}

#[derive(Debug)]
struct Oracle {
    bytes: BTreeMap<String, Vec<u8>>,
    clock_by_utc: BTreeMap<String, i64>,
    dates: Vec<String>,
    features: BTreeMap<String, WindFitLocalFeatures>,
    fingerprint: RuntimeFingerprint,
    formatted_utc: BTreeMap<i64, String>,
    midnights: BTreeMap<(String, bool), i64>,
    preimages: BTreeMap<String, Preimage>,
    result: ExpectedResult,
}

struct Values<'a> {
    oracle: &'a Oracle,
    trace: RefCell<Vec<String>>,
}

impl ApiMaintenanceReceiptCanonicalValues for Values<'_> {
    /// Resolve only clocks emitted by the compiled original calendar fixture.
    fn canonical_utc_epoch_milliseconds(&self, value: &str) -> Option<i64> {
        self.oracle.clock_by_utc.get(value).copied()
    }

    /// Return only original Node hashes for the exact manifest and report preimages.
    fn sha256(&self, bytes: &[u8]) -> Option<Sha256Digest> {
        self.oracle
            .preimages
            .values()
            .find(|preimage| preimage.bytes == bytes)
            .map(|preimage| Sha256Digest::admitted(&preimage.sha256))
    }

    /// Fit input admission does not create maintenance HTTP receipts.
    fn unsigned_receipt_sha256(
        &self,
        _: &[(&str, MaintenanceReceiptScalar<'_>)],
    ) -> Option<Sha256Digest> {
        unreachable!("wind fit input does not encode an HTTP receipt")
    }
}

impl ApiMaintenanceShadowEvidenceValues for Values<'_> {
    /// Supply the exact typed input only for the in-memory command's fixture bytes.
    fn decode_shadow_json(&self, bytes: &[u8]) -> Option<JsonValue> {
        (bytes == FIXTURE_INPUT)
            .then(|| input_document(self.oracle, self.oracle.result.total_row_count))
    }

    /// The fit path uses only recursively sorted canonical output.
    fn encode_shadow_ordered_json(&self, _: &JsonValue) -> Option<Vec<u8>> {
        unreachable!("wind fit input does not encode ordered source JSON")
    }

    /// No comparator payload exists in the archive input.
    fn decode_shadow_comparator_base64(&self, _: &[u16]) -> Option<Vec<u8>> {
        unreachable!()
    }

    /// Resolve the three exact canonical documents independently emitted by Node.
    fn encode_shadow_sorted_json(&self, doc: &JsonValue) -> Option<Vec<u8>> {
        let label = if field(doc, "epochWitnessSha256").is_some() {
            let fixture = input_document(self.oracle, self.oracle.result.total_row_count);
            assert_eq!(Some(doc), field(&fixture, "manifest"));
            "manifest"
        } else if field(doc, "confirmationOpened").is_some() {
            assert_publication(doc, &self.oracle.result);
            "publication-bytes"
        } else if field(doc, "failedGates").is_some() {
            assert_insufficient(doc, &self.oracle.result);
            if field(doc, "reportSha256").is_some() {
                "insufficient-bytes"
            } else {
                "insufficient"
            }
        } else {
            return None;
        };
        self.trace.borrow_mut().push(label.to_owned());
        // sealed documents use full bytes while unsigned material is hashable preimage data
        if let Some(key) = label.strip_suffix("-bytes") {
            return self.oracle.bytes.get(key).cloned();
        }
        self.oracle
            .preimages
            .get(label)
            .map(|preimage| preimage.bytes.clone())
    }

    /// No maintenance package is decoded on the fit-input path.
    fn decode_shadow_maintenance_package(
        &self,
        _: &JsonValue,
    ) -> Option<weather_contracts::ForecastAdjustmentMaintenanceRuntimePackage> {
        unreachable!()
    }

    /// No rain artifact is decoded on the wind-fit path.
    fn decode_shadow_rain_artifact(
        &self,
        _: &JsonValue,
    ) -> Option<weather_contracts::RainHurdleWindPortableArtifact> {
        unreachable!()
    }

    /// Wind input admission never examines legacy runtime code.
    fn check_legacy_wind_runtime_value(&self, _: &[u16]) -> Option<()> {
        unreachable!()
    }
}

struct Calendar<'a>(&'a Oracle);

impl WindMaintenanceLocalCalendar for Calendar<'_> {
    /// Return only the three site-midnight boundaries emitted by Node.
    fn wind_local_midnight_milliseconds(&self, date: &str, next_day: bool) -> Option<i64> {
        self.0.midnights.get(&(date.to_owned(), next_day)).copied()
    }
}

impl WindFitInputCalendar for Calendar<'_> {
    /// Use the compiled epoch position as the admitted local-day ordinal.
    fn wind_fit_date_ordinal(&self, date: &str) -> Option<i64> {
        self.0
            .dates
            .iter()
            .position(|candidate| candidate == date)
            .map(|index| index as i64)
    }

    /// Add only within the compiled original 402-date epoch.
    fn wind_fit_add_local_days(&self, date: &str, days: u16) -> Option<String> {
        let start = self
            .0
            .dates
            .iter()
            .position(|candidate| candidate == date)?;
        self.0.dates.get(start + usize::from(days)).cloned()
    }

    /// Format only UTC instants emitted by the independent oracle.
    fn wind_fit_utc_at(&self, ms: i64) -> Option<String> {
        self.0.formatted_utc.get(&ms).cloned()
    }

    /// Return exact original local features for a retained UTC noon.
    fn wind_fit_local_features(&self, utc: &str, ms: i64) -> Option<WindFitLocalFeatures> {
        (self.0.clock_by_utc.get(utc) == Some(&ms)).then(|| self.0.features.get(utc).cloned())?
    }

    /// Preserve the Node runtime calendar identity used by the fixture.
    fn wind_fit_runtime_fingerprint(&self) -> RuntimeFingerprint {
        self.0.fingerprint.clone()
    }
}

/// Run the fixed-path command lifecycle in memory and compare the ordinary Node refusal.
#[test]
fn matches_node_missing_receipt_insufficiency() {
    let oracle = load_oracle();
    let values = Values {
        oracle: &oracle,
        trace: RefCell::new(Vec::new()),
    };
    let mut io = FixtureIo::default();
    let invalid = run_wind_maintenance_cli(&[], &values, &Calendar(&oracle), &mut io);
    assert_eq!(invalid, Err(WindMaintenanceCliError::Arguments));
    assert!(io.trace.is_empty());
    let output = run_wind_maintenance_cli(
        &["--fit-only".to_owned()],
        &values,
        &Calendar(&oracle),
        &mut io,
    );
    assert_eq!(output, Ok(2));
    assert_eq!(wind_maintenance_process_exit_code(&output), 0);
    assert_eq!(io.output, oracle.bytes["publication"]);
    assert_eq!(
        io.trace,
        [
            "lstat",
            "open",
            "stat",
            "read",
            "close_input",
            "create",
            "write",
            "sync",
            "close_output",
            "stdout"
        ],
    );
    assert_eq!(
        values.trace.into_inner(),
        [
            "manifest",
            "insufficient",
            "insufficient-bytes",
            "publication-bytes",
        ]
    );
}

#[derive(Default)]
struct FixtureIo {
    trace: Vec<&'static str>,
    output: Vec<u8>,
}

const FIXTURE_METADATA: WindMaintenanceFileMetadata = WindMaintenanceFileMetadata {
    device: 7,
    inode: 11,
    regular: true,
    size: FIXTURE_INPUT.len() as u64,
    symlink: false,
};

impl WindMaintenanceCliIo for FixtureIo {
    type Error = ();
    type Reader = ();
    type Writer = ();

    /// Inspect only the original fixed input.
    fn lstat(&mut self, path: &'static str) -> Result<WindMaintenanceFileMetadata, ()> {
        assert_eq!(path, "/input/data/wind.json");
        self.trace.push("lstat");
        Ok(FIXTURE_METADATA)
    }

    /// Open the same fixture path; no native file is touched.
    fn open_read_no_follow(&mut self, path: &'static str) -> Result<(), ()> {
        assert_eq!(path, "/input/data/wind.json");
        self.trace.push("open");
        Ok(())
    }

    /// Match the same fixture descriptor metadata.
    fn reader_metadata(&mut self, _: &mut ()) -> Result<WindMaintenanceFileMetadata, ()> {
        self.trace.push("stat");
        Ok(FIXTURE_METADATA)
    }

    /// Preserve the exact original input ceiling.
    fn read_to_end(&mut self, _: &mut (), maximum_bytes: usize) -> Result<Vec<u8>, ()> {
        assert_eq!(maximum_bytes, 512 * 1_024 * 1_024);
        self.trace.push("read");
        Ok(FIXTURE_INPUT.to_vec())
    }

    /// Record that the input closed before fitting and output.
    fn close_reader(&mut self, _: ()) -> Result<(), ()> {
        self.trace.push("close_input");
        Ok(())
    }

    /// Destination and mode match the original process.
    fn create_new(&mut self, path: &'static str, mode: u32) -> Result<(), ()> {
        assert_eq!((path, mode), ("/output/wind.json", 0o600));
        self.trace.push("create");
        Ok(())
    }

    /// Retain the exact output content.
    fn write_all(&mut self, _: &mut (), bytes: &[u8]) -> Result<(), ()> {
        self.trace.push("write");
        self.output.extend(bytes);
        Ok(())
    }

    /// Record the output durability step.
    fn sync_all(&mut self, _: &mut ()) -> Result<(), ()> {
        self.trace.push("sync");
        Ok(())
    }

    /// Record the output close before stdout.
    fn close_writer(&mut self, _: ()) -> Result<(), ()> {
        self.trace.push("close_output");
        Ok(())
    }

    /// Check the same exact bytes reach stdout.
    fn write_stdout(&mut self, bytes: &[u8]) -> Result<(), ()> {
        self.trace.push("stdout");
        assert_eq!(self.output, bytes);
        Ok(())
    }
}

/// Report missing opened-member evidence before a malformed row can be projected.
#[test]
fn checks_member_custody_before_row_projection() {
    let oracle = load_oracle();
    let values = Values {
        oracle: &oracle,
        trace: RefCell::new(Vec::new()),
    };
    let mut doc = input_document(&oracle, oracle.result.total_row_count);
    let JsonValue::Object(fields) = &mut doc else {
        panic!("fixture object");
    };
    // combine two ordinary failures to prove the original precedence
    for (key, node) in fields {
        match String::from_utf16(key).expect("fixture keys").as_str() {
            "openedMembers" => *node = JsonValue::Array(Vec::new()),
            "rows" => *node = JsonValue::Array(vec![JsonValue::Null]),
            _ => {}
        }
    }
    let failure = run_wind_maintenance_document(&values, &Calendar(&oracle), &[], doc);
    assert_eq!(
        failure,
        Err(WindFitInputError::Cutoff(
            WindMaintenanceCutoffError::MemberNotOpenedBeforeCutoff
        ))
    );
}

/// Reject a declared archive row total that differs from its 402 one-row members.
#[test]
fn rejects_archive_metadata_row_count_drift() {
    let oracle = load_oracle();
    let values = Values {
        oracle: &oracle,
        trace: RefCell::new(Vec::new()),
    };
    let error = run_wind_maintenance_document(
        &values,
        &Calendar(&oracle),
        &[],
        input_document(&oracle, oracle.result.total_row_count - 1),
    )
    .expect_err("declared and derived member row counts must agree");
    assert_eq!(error, WindFitInputError::Manifest);
    assert!(values.trace.into_inner().is_empty());
}

/// Load dates, calendar projections, canonical bytes, and hashes from compiled Node.
fn load_oracle() -> Oracle {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/wind_maintenance_input_node_oracle.mjs");
    let output = Command::new("node")
        .arg(path)
        .output()
        .expect("compiled Node wind-input oracle starts");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("oracle output is UTF-8");
    let mut bytes = BTreeMap::new();
    let mut clock_by_utc = BTreeMap::new();
    let mut dates = Vec::new();
    let mut features = BTreeMap::new();
    let mut fingerprint = None;
    let mut formatted_utc = BTreeMap::new();
    let mut midnights = BTreeMap::new();
    let mut preimages = BTreeMap::new();
    let mut result = None;
    // retain all explicit oracle rows without implementing calendar or JSON semantics locally
    for line in stdout.lines() {
        let fields = line.split('\t').collect::<Vec<_>>();
        match fields.as_slice() {
            ["DATE", index, date] => {
                assert_eq!(index.parse::<usize>().expect("date index"), dates.len());
                dates.push((*date).to_owned());
            }
            ["NOON", _date, utc, ms, local_date, month, season, daypart] => {
                let ms = ms.parse::<i64>().expect("noon epoch");
                clock_by_utc.insert((*utc).to_owned(), ms);
                features.insert(
                    (*utc).to_owned(),
                    WindFitLocalFeatures {
                        local_date: (*local_date).to_owned(),
                        calendar: AdmittedWindRowCalendar {
                            daypart: daypart_value(daypart),
                            month: month.parse::<u64>().expect("local month"),
                            season: season_value(season),
                        },
                    },
                );
            }
            ["INSTANT", utc, ms, local_date, month, season, daypart] => {
                clock_by_utc.insert((*utc).to_owned(), ms.parse().expect("instant epoch"));
                // retain features only for the one row instant, not the receipt clock
                if !local_date.is_empty() {
                    features.insert(
                        (*utc).to_owned(),
                        WindFitLocalFeatures {
                            local_date: (*local_date).to_owned(),
                            calendar: AdmittedWindRowCalendar {
                                daypart: daypart_value(daypart),
                                month: month.parse().expect("instant month"),
                                season: season_value(season),
                            },
                        },
                    );
                }
            }
            ["MIDNIGHT", date, next_day, ms, utc] => {
                let ms = ms.parse::<i64>().expect("midnight epoch");
                let next_day = next_day.parse::<bool>().expect("midnight flag");
                midnights.insert(((*date).to_owned(), next_day), ms);
                clock_by_utc.insert((*utc).to_owned(), ms);
                formatted_utc.insert(ms, (*utc).to_owned());
            }
            ["UTC", ms, utc] => {
                let ms = ms.parse::<i64>().expect("UTC epoch");
                clock_by_utc.insert((*utc).to_owned(), ms);
                formatted_utc.insert(ms, (*utc).to_owned());
            }
            ["FINGERPRINT", icu, tzdata] => {
                fingerprint = Some(RuntimeFingerprint {
                    icuVersion: (*icu).to_owned(),
                    tzdataVersion: (*tzdata).to_owned(),
                });
            }
            ["PREIMAGE", label, encoded, sha256] => {
                preimages.insert(
                    (*label).to_owned(),
                    Preimage {
                        bytes: decode_hex(encoded),
                        sha256: (*sha256).to_owned(),
                    },
                );
            }
            ["BYTES", label, encoded] => {
                bytes.insert((*label).to_owned(), decode_hex(encoded));
            }
            ["RESULT", contract, state, gate, report, snapshot, total] => {
                result = Some(ExpectedResult {
                    contract_version: (*contract).to_owned(),
                    state: (*state).to_owned(),
                    failed_gate: (*gate).to_owned(),
                    report_sha256: (*report).to_owned(),
                    snapshot_sha256: (*snapshot).to_owned(),
                    total_row_count: total.parse().expect("manifest row count"),
                });
            }
            _ => panic!("unexpected oracle row: {line}"),
        }
    }
    Oracle {
        bytes,
        clock_by_utc,
        dates,
        features,
        fingerprint: fingerprint.expect("runtime fingerprint"),
        formatted_utc,
        midnights,
        preimages,
        result: result.expect("result projection"),
    }
}

/// Construct the raw closed archive input without reading any member files.
fn input_document(oracle: &Oracle, total_row_count: usize) -> JsonValue {
    let members = oracle
        .dates
        .iter()
        .enumerate()
        .map(|(index, date)| {
            let sha256 = format!("{:064x}", index + 1);
            object(vec![
                ("localDate", string(date)),
                ("maxValidAt", string(VALID_AT)),
                ("minValidAt", string(VALID_AT)),
                (
                    "path",
                    string(&format!("archive-members/{date}/forecast/{sha256}")),
                ),
                ("plaintextBytes", JsonValue::Number(1.0)),
                ("recordKind", string("actual_best_match")),
                ("rowCount", JsonValue::Number(1.0)),
                ("sha256", string(&sha256)),
                ("sizeBytes", JsonValue::Number(1.0)),
                ("stationKey", JsonValue::Null),
            ])
        })
        .collect::<Vec<_>>();
    let manifest = object(vec![
        ("aggregationContractSha256", string(&"1".repeat(64))),
        (
            "contractVersion",
            string("adjustment-wind-archive-fit-manifest/v2"),
        ),
        ("coordinateManifestSha256", string(&"2".repeat(64))),
        ("epochWitnessSha256", string(&"3".repeat(64))),
        ("fromLocalDate", string(&oracle.dates[0])),
        ("historyRootSha256", string(&"4".repeat(64))),
        ("members", JsonValue::Array(members)),
        ("metricEligibilitySha256", string(&"5".repeat(64))),
        ("sourceLineageSha256", string(&"6".repeat(64))),
        ("spatialWeightsSha256", string(&"7".repeat(64))),
        ("stationManifestSha256", string(&"8".repeat(64))),
        ("toLocalDate", string(&oracle.dates[401])),
        ("totalRowCount", JsonValue::Number(total_row_count as f64)),
    ]);
    let opened = oracle
        .dates
        .iter()
        .enumerate()
        .map(|(index, _)| {
            object(vec![
                ("memberSha256", string(&format!("{:064x}", index + 1))),
                ("maximumSourceReceiptAt", string(OPENED_AT)),
                ("maximumTargetReceiptAt", string(OPENED_AT)),
            ])
        })
        .collect();
    object(vec![
        ("contractVersion", string("wind-maintenance-fit-input/v2")),
        ("manifest", manifest),
        (
            "rows",
            JsonValue::Array(vec![object(vec![
                ("receivedAt", JsonValue::Null),
                ("recordKind", string("fixed_lead_anchor")),
                ("validAt", string(VALID_AT)),
            ])]),
        ),
        (
            "snapshotManifestSha256",
            string(&oracle.result.snapshot_sha256),
        ),
        ("dueMonth", string(DUE_MONTH)),
        ("openedMembers", JsonValue::Array(opened)),
    ])
}

/// Verify the sanitized insufficiency document before supplying Node's canonical bytes.
fn assert_insufficient(doc: &JsonValue, expected: &ExpectedResult) {
    assert_eq!(
        text_field(doc, "contractVersion"),
        expected.contract_version
    );
    assert_eq!(text_field(doc, "state"), expected.state);
    assert_eq!(
        text_field(doc, "snapshotManifestSha256"),
        expected.snapshot_sha256
    );
    let JsonValue::Array(gates) = field(doc, "failedGates").expect("failed gates") else {
        panic!("failed gates must be an array")
    };
    assert_eq!(gates, &[string(&expected.failed_gate)]);
    // unsigned and sealed report documents differ only by this self-address
    if let Some(JsonValue::String(_)) = field(doc, "reportSha256") {
        assert_eq!(text_field(doc, "reportSha256"), expected.report_sha256);
    }
}

/// Verify the exact CLI no-candidate wrapper fields before canonical encoding.
fn assert_publication(doc: &JsonValue, expected: &ExpectedResult) {
    assert_eq!(
        text_field(doc, "contractVersion"),
        "wind-maintenance-fit/v2"
    );
    assert_eq!(text_field(doc, "state"), "no_candidate");
    assert_eq!(text_field(doc, "dueMonth"), DUE_MONTH);
    assert_eq!(
        text_field(doc, "reason"),
        "insufficient_development_support"
    );
    assert_eq!(text_field(doc, "reportSha256"), expected.report_sha256);
    assert_eq!(
        field(doc, "confirmationOpened"),
        Some(&JsonValue::Boolean(false))
    );
}

/// Construct one ordered dependency-free JSON object.
fn object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(name, value)| (name.encode_utf16().collect(), value))
            .collect(),
    )
}

/// Construct one dependency-free JSON string.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
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
fn text_field(value: &JsonValue, key: &str) -> String {
    let Some(JsonValue::String(units)) = field(value, key) else {
        panic!("missing text field {key}")
    };
    String::from_utf16(units).expect("fixture text is valid UTF-16")
}

/// Decode bounded oracle hexadecimal without implementing a production JSON codec.
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

/// Map one original local daypart wire value.
fn daypart_value(value: &str) -> Daypart {
    match value.as_bytes() {
        b"afternoon" => Daypart::Afternoon,
        b"evening" => Daypart::Evening,
        b"morning" => Daypart::Morning,
        b"night" => Daypart::Night,
        _ => panic!("unknown daypart"),
    }
}

/// Map one original meteorological season wire value.
fn season_value(value: &str) -> Season {
    match value.as_bytes() {
        b"autumn" => Season::Autumn,
        b"spring" => Season::Spring,
        b"summer" => Season::Summer,
        b"winter" => Season::Winter,
        _ => panic!("unknown season"),
    }
}
