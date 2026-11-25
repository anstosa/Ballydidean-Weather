use std::collections::VecDeque;
use std::path::PathBuf;
use std::process::Command;

use weather_api::{
    CanonicalInstant, CurrentProjectionError, CurrentTimeResolver, MaintenanceSourceCanonicalCodec,
    MaintenanceSourceField, MaintenanceSourceProjection, MaintenanceSourceProjectionHeader,
    MaintenanceSourceValue, build_temperature_maintenance_source_projection,
    build_wind_maintenance_source_projection, prepare_maintenance_source,
};
use weather_contracts::{
    EcmwfTemperatureCanaryHour, EcmwfTemperatureCanaryRecentErrorState,
    EcmwfTemperatureCanaryRecentErrorStateCohortLiteral, EcmwfTemperatureCanaryRun,
    EcmwfTemperatureCanaryRunAdapterVersionLiteral, EcmwfTemperatureCanaryRunModelCycleEnum,
    EcmwfTemperatureCanaryRunStateStatusEnum, EcmwfTemperatureCanaryRunUpstreamModelLiteral,
    EcmwfTemperatureCanarySidecar, JsDateOrString, JsonValue, MaintenanceShadowSourceIdentity,
    Nullable, SourceKind, WeatherRecordRow,
};
use weather_database::Sha256Digest;

const REGISTRATION_SHA256: &str =
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const CANDIDATE_SHA256: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const SOURCE_SHA256: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const DUE_KEY: &str = "capture/2026-09-01T00:35:00.000Z";
const ISSUED_AT: &str = "2026-09-01T00:35:00.000Z";
const REFERENCE_AT: &str = "2026-09-01T00:00:00.000Z";
const RECEIVED_AT: &str = "2026-09-01T00:05:00.000Z";
const BASE_EPOCH_MS: i64 = 1_788_220_800_000;

/// Exact canonical witnesses emitted by the original compiled Node implementation.
struct NodeOracle {
    input_sha256: String,
    receipt_sha256: String,
    row_bytes: Vec<Vec<u8>>,
    row_sha256: Vec<String>,
    source_bytes: Vec<u8>,
}

/// Fixture resolver admitting only the explicitly constructed UTC values.
struct FixtureTime {
    values: Vec<(Vec<u16>, CanonicalInstant)>,
}

impl CurrentTimeResolver for FixtureTime {
    /// Resolve one exact fixture wire value without implementing a date codec.
    fn resolve(&self, value: &JsDateOrString) -> Result<CanonicalInstant, CurrentProjectionError> {
        // admit only the same raw JavaScript strings used by the Node fixture
        if let JsDateOrString::String(value) = value {
            return self
                .values
                .iter()
                .find_map(|(source, instant)| (source == value).then(|| instant.clone()))
                .ok_or_else(|| CurrentProjectionError {
                    code: "maintenance_source_projection.time",
                    detail: "fixture time was not admitted".to_owned(),
                });
        }
        Err(CurrentProjectionError {
            code: "maintenance_source_projection.time",
            detail: "fixture time was not a string".to_owned(),
        })
    }
}

/// Deterministic codec returning Node bytes while retaining every Rust identity preimage.
struct FixtureCodec {
    digests: VecDeque<Sha256Digest>,
    preimages: Vec<Vec<u8>>,
    rows: VecDeque<Vec<u8>>,
    source: Vec<u8>,
}

impl MaintenanceSourceCanonicalCodec for FixtureCodec {
    /// Return the original full canonical projection bytes.
    fn encode_source(
        &mut self,
        _projection: &weather_api::MaintenanceSourceProjection<'_>,
    ) -> Option<Vec<u8>> {
        Some(self.source.clone())
    }

    /// Compare every projected field and key order to the Node fixture before releasing its bytes.
    fn encode_row(&mut self, fields: &[MaintenanceSourceField]) -> Option<Vec<u8>> {
        let expected = self.rows.pop_front()?;
        assert_eq!(fixture_row_json(fields).as_bytes(), expected.as_slice());
        Some(expected)
    }

    /// Retain the exact concatenated Rust preimage and return the paired Node digest.
    fn sha256(&mut self, parts: &[&[u8]]) -> Option<Sha256Digest> {
        self.preimages.push(parts.concat());
        self.digests.pop_front()
    }
}

/// Match a complete 168-hour Rust projection and all identity preimages to compiled Node.
#[test]
fn matches_compiled_node_wind_source_projection_and_identity() {
    let oracle = run_node_oracle("wind", 168);
    let rows = wind_rows();
    let time = fixture_time();
    let issued_at = instant(BASE_EPOCH_MS + 35 * 60_000, ISSUED_AT);
    let projection = build_wind_maintenance_source_projection(header(&issued_at), &rows, &time)
        .expect("build the complete Node-matching source projection");

    assert_eq!(projection.family(), "wind");
    assert_eq!(projection.row_count(), 168);
    // compare every projected field against the same deterministic Node inputs
    for lead_hours in 1..=168 {
        assert_wind_row(
            projection
                .row_fields(usize::try_from(lead_hours - 1).expect("fixture row index"))
                .expect("complete wind row"),
            lead_hours,
        );
    }
    assert_source_identity(projection, &oracle, &time);
}

/// Match all 12 paired temperature rows, preserving original comparator lookup and binary64.
#[test]
fn matches_compiled_node_temperature_source_projection_and_identity() {
    let oracle = run_node_oracle("temperature", 12);
    let best_match = (1..=12)
        .rev()
        .map(|lead| {
            let mut row = wind_row(lead);
            // preserve negative zero and reverse comparator order to exercise time-key matching
            row.temperatureC = Nullable::Value(if lead == 1 {
                -0.0
            } else {
                10.0 + (lead - 1) as f64 / 4.0
            });
            row
        })
        .collect::<Vec<_>>();
    let sidecar = temperature_sidecar();
    let time = fixture_time();
    let issued_at = instant(BASE_EPOCH_MS + 35 * 60_000, ISSUED_AT);
    let projection = build_temperature_maintenance_source_projection(
        header(&issued_at),
        &best_match,
        &sidecar,
        &time,
    )
    .expect("build the complete Node-matching temperature projection");
    assert_eq!(projection.family(), "temperature");
    assert_eq!(projection.row_count(), 12);
    assert_source_identity(projection, &oracle, &time);
}

/// Bind source identity preimages using the compiled Node bytes and compare all original rows.
fn assert_source_identity(
    projection: MaintenanceSourceProjection<'_>,
    oracle: &NodeOracle,
    time: &FixtureTime,
) {
    let count = projection.row_count();
    let family = projection.family();

    let mut digests = VecDeque::new();
    digests.push_back(Sha256Digest::admitted(oracle.input_sha256.clone()));
    // preserve the exact Node source-row digest order before the receipt digest
    for digest in &oracle.row_sha256 {
        digests.push_back(Sha256Digest::admitted(digest.clone()));
    }
    digests.push_back(Sha256Digest::admitted(oracle.receipt_sha256.clone()));
    let mut codec = FixtureCodec {
        digests,
        preimages: Vec::new(),
        rows: oracle.row_bytes.clone().into(),
        source: oracle.source_bytes.clone(),
    };

    let epoch = instant(BASE_EPOCH_MS - 96 * 3_600_000, "2026-08-28T00:00:00.000Z");
    let prepared = prepare_maintenance_source(projection, &epoch, time, &mut codec)
        .expect("identify the exact Node source projection");

    assert_eq!(prepared.bytes, oracle.source_bytes);
    assert_eq!(prepared.identity, oracle_identity(oracle));
    assert!(codec.rows.is_empty());
    assert!(codec.digests.is_empty());
    assert_eq!(codec.preimages.len(), count + 2);
    assert_eq!(codec.preimages[0], oracle.source_bytes);
    // verify every row hash preimage instead of sampling only the first and last rows
    for (index, row) in oracle.row_bytes.iter().enumerate() {
        assert_eq!(codec.preimages[index + 1], row_preimage(family, row));
    }
    assert_eq!(codec.preimages[count + 1], receipt_preimage(oracle, count));
}

/// Run the compiled Node owner and decode its line-oriented exact-byte witnesses.
fn run_node_oracle(family: &str, count: usize) -> NodeOracle {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/maintenance-source-projection-node-oracle.mjs");
    let output = Command::new("node")
        .arg(script)
        .arg(family)
        .output()
        .expect("run maintenance source projection Node oracle");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = String::from_utf8(output.stdout).expect("Node oracle emits ASCII witnesses");
    let mut source_bytes = None;
    let mut input_sha256 = None;
    let mut receipt_sha256 = None;
    let mut row_bytes = vec![Vec::new(); count];
    let mut row_sha256 = vec![String::new(); count];

    // decode each tagged witness without adding a JSON parser to the Rust crate
    for line in output.lines() {
        if let Some(value) = line.strip_prefix("source=") {
            source_bytes = Some(decode_hex(value));
        } else if let Some((index, value)) = indexed_value(line, "row:") {
            row_bytes[index] = decode_hex(value);
        } else if let Some(value) = line.strip_prefix("input=") {
            input_sha256 = Some(value.to_owned());
        } else if let Some(value) = line.strip_prefix("receipt=") {
            receipt_sha256 = Some(value.to_owned());
        } else if let Some((index, value)) = indexed_value(line, "row-sha:") {
            row_sha256[index] = value.to_owned();
        } else {
            panic!("unexpected Node oracle line");
        }
    }
    assert!(row_bytes.iter().all(|value| !value.is_empty()));
    assert!(row_sha256.iter().all(|value| value.len() == 64));
    NodeOracle {
        input_sha256: input_sha256.expect("input digest"),
        receipt_sha256: receipt_sha256.expect("receipt digest"),
        row_bytes,
        row_sha256,
        source_bytes: source_bytes.expect("source bytes"),
    }
}

/// Split one indexed `tag:index=value` oracle witness.
fn indexed_value<'a>(line: &'a str, tag: &str) -> Option<(usize, &'a str)> {
    let value = line.strip_prefix(tag)?;
    let (index, value) = value.split_once('=')?;
    Some((index.parse().expect("bounded fixture index"), value))
}

/// Decode lowercase hexadecimal bytes with no package dependency.
fn decode_hex(value: &str) -> Vec<u8> {
    assert_eq!(value.len() % 2, 0);
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| (hex_digit(pair[0]) << 4) | hex_digit(pair[1]))
        .collect()
}

/// Decode one fixture hexadecimal digit.
fn hex_digit(value: u8) -> u8 {
    match value {
        b'0'..=b'9' => value - b'0',
        b'a'..=b'f' => value - b'a' + 10,
        _ => panic!("noncanonical fixture hexadecimal digit"),
    }
}

/// Construct the exact generated identity expected from the Rust helper.
fn oracle_identity(oracle: &NodeOracle) -> MaintenanceShadowSourceIdentity {
    MaintenanceShadowSourceIdentity {
        inputSha256: oracle.input_sha256.clone(),
        sourceReceiptSha256: oracle.receipt_sha256.clone(),
        sourceRowSha256: oracle.row_sha256.clone(),
    }
}

/// Construct one Node source-row hash preimage.
fn row_preimage(family: &str, row: &[u8]) -> Vec<u8> {
    [
        b"adjustment-shadow-source-row/v1\n".as_slice(),
        family.as_bytes(),
        b"\n",
        row,
        b"\n",
    ]
    .concat()
}

/// Construct the Node source receipt hash preimage.
fn receipt_preimage(oracle: &NodeOracle, count: usize) -> Vec<u8> {
    [
        "adjustment-shadow-source-receipt/v1",
        REGISTRATION_SHA256,
        CANDIDATE_SHA256,
        SOURCE_SHA256,
        DUE_KEY,
        ISSUED_AT,
        "2026-09-01T01:00:00.000Z",
        &valid_at(u64::try_from(count).expect("fixture row count")),
        &count.to_string(),
        &oracle.input_sha256,
    ]
    .join("\n")
    .into_bytes()
}

/// Project only the ASCII test fixture into bytes for direct comparison with Node JSON.
fn fixture_row_json(fields: &[MaintenanceSourceField]) -> String {
    let mut encoded = String::from("{");
    // emit known simple fixture values without adding a runtime JSON codec
    for (index, (key, value)) in fields.iter().enumerate() {
        // preserve the fixture field order and the boundary between fields
        if index > 0 {
            encoded.push(',');
        }
        fixture_json_string(&mut encoded, key);
        encoded.push(':');
        match value {
            MaintenanceSourceValue::Text(text) => fixture_json_string(&mut encoded, text),
            MaintenanceSourceValue::Utf16(units) => fixture_json_string(
                &mut encoded,
                &String::from_utf16(units).expect("known ASCII fixture text"),
            ),
            MaintenanceSourceValue::Number(number) => encoded.push_str(&number.to_string()),
            MaintenanceSourceValue::Null => encoded.push_str("null"),
        }
    }
    encoded.push('}');
    encoded
}

/// Quote only known fixture ASCII whose Node rendering has no escapes.
fn fixture_json_string(encoded: &mut String, value: &str) {
    assert!(
        value
            .bytes()
            .all(|byte| (b' '..=b'~').contains(&byte) && !matches!(byte, b'"' | b'\\'))
    );
    encoded.push('"');
    encoded.push_str(value);
    encoded.push('"');
}

/// Construct the known fixture header from generated candidate/lifecycle identities.
fn header(issued_at: &CanonicalInstant) -> MaintenanceSourceProjectionHeader<'_> {
    MaintenanceSourceProjectionHeader {
        registration_sha256: REGISTRATION_SHA256,
        candidate_sha256: CANDIDATE_SHA256,
        source_sha256: SOURCE_SHA256,
        due_key: DUE_KEY,
        issued_at,
    }
}

/// Construct the same fixed cold ECMWF source and finite raw metrics as the Node fixture.
fn temperature_sidecar() -> EcmwfTemperatureCanarySidecar {
    EcmwfTemperatureCanarySidecar {
        hours: (0..12_u64)
            .map(|index| EcmwfTemperatureCanaryHour {
                contentHash: format!("{:064x}", index + 256),
                modelLeadHours: index + 1,
                rawRelativeHumidityPercent: if index.is_multiple_of(4) {
                    Nullable::Null
                } else {
                    Nullable::Value(70.0 + index as f64)
                },
                rawTemperatureC: 11.0 + index as f64 / 4.0,
                rawWindSpeedMps: if index.is_multiple_of(3) {
                    Nullable::Null
                } else {
                    Nullable::Value(2.0)
                },
                validAt: valid_at(index + 1),
            })
            .collect(),
        run: EcmwfTemperatureCanaryRun {
            adapterVersion: EcmwfTemperatureCanaryRunAdapterVersionLiteral,
            firstReceivedAt: RECEIVED_AT.to_owned(),
            id: "12".to_owned(),
            modelCycle: EcmwfTemperatureCanaryRunModelCycleEnum::Value49r1,
            providerResponseSha256: "e".repeat(64),
            recentErrorState: EcmwfTemperatureCanaryRecentErrorState {
                b24C: Nullable::Null,
                b72C: Nullable::Null,
                cohort: EcmwfTemperatureCanaryRecentErrorStateCohortLiteral,
                localDates: 0,
                mad72C: Nullable::Null,
                maximumSourceRunInitializedAt: Nullable::Null,
                maximumSourceValidAt: Nullable::Null,
                n24: 0,
                n72: 0,
                sourceKeys: Vec::new(),
                supported: false,
                targetRunInitializedAt: REFERENCE_AT.to_owned(),
                windowEndValidAt: REFERENCE_AT.to_owned(),
            },
            runInitializedAt: REFERENCE_AT.to_owned(),
            stateReason: "no_causal_forecast_observation_pairs".to_owned(),
            stateStatus: EcmwfTemperatureCanaryRunStateStatusEnum::Cold,
            upstreamModel: EcmwfTemperatureCanaryRunUpstreamModelLiteral,
        },
    }
}

/// Assert the exact field order and edge-row values derived from the Node projection.
fn assert_wind_row(fields: &[MaintenanceSourceField], lead_hours: u64) {
    assert_eq!(
        fields.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
        [
            "validAt",
            "leadHours",
            "modelLeadHours",
            "referenceAt",
            "receivedAt",
            "sourceSha256",
            "contentSha256",
            "adapterVersion",
            "contractEpoch",
            "dataset",
            "providerKey",
            "sourceKey",
            "sourceConfigFingerprint",
            "upstreamModel",
            "revisionCount",
            "windSpeedMps64",
            "windGustMps64",
        ]
    );
    assert_eq!(
        fields[0].1,
        MaintenanceSourceValue::Text(valid_at(lead_hours))
    );
    assert_eq!(fields[1].1, MaintenanceSourceValue::Number(lead_hours));
    assert_eq!(fields[2].1, MaintenanceSourceValue::Number(lead_hours));
    assert_eq!(
        fields[3].1,
        MaintenanceSourceValue::Text(REFERENCE_AT.to_owned())
    );
    assert_eq!(
        fields[4].1,
        MaintenanceSourceValue::Text(RECEIVED_AT.to_owned())
    );
    assert_eq!(
        fields[5].1,
        MaintenanceSourceValue::Text(SOURCE_SHA256.to_owned())
    );
    assert_eq!(
        fields[6].1,
        MaintenanceSourceValue::Text(format!("{lead_hours:064x}"))
    );
    assert_eq!(
        fields[7].1,
        MaintenanceSourceValue::Text("open-meteo-forecast-daily/v4".to_owned())
    );
    assert_eq!(
        fields[8].1,
        MaintenanceSourceValue::Text("legacy-v4/fixture".to_owned())
    );
    assert_eq!(
        fields[9].1,
        MaintenanceSourceValue::Utf16(text("best_match"))
    );
    assert_eq!(
        fields[10].1,
        MaintenanceSourceValue::Text("open-meteo".to_owned())
    );
    assert_eq!(
        fields[11].1,
        MaintenanceSourceValue::Text("open-meteo-forecast-v4".to_owned())
    );
    assert_eq!(fields[12].1, MaintenanceSourceValue::Text("d".repeat(64)));
    assert_eq!(
        fields[13].1,
        MaintenanceSourceValue::Text("best_match".to_owned())
    );
    assert_eq!(fields[14].1, MaintenanceSourceValue::Number(lead_hours - 1));
    assert_eq!(
        fields[15].1,
        MaintenanceSourceValue::Text(binary64_hex(1.0 + lead_hours as f64 / 100.0))
    );
    let gust = if lead_hours.is_multiple_of(24) {
        MaintenanceSourceValue::Null
    } else {
        MaintenanceSourceValue::Text(binary64_hex(2.0 + lead_hours as f64 / 100.0))
    };
    assert_eq!(fields[16].1, gust);
}

/// Build the exact complete Best Match row vector consumed by both implementations.
fn wind_rows() -> Vec<WeatherRecordRow> {
    (1..=168).map(wind_row).collect()
}

/// Build one generated forecast row for an exact hourly lead.
fn wind_row(lead_hours: u64) -> WeatherRecordRow {
    let valid_at = valid_at(lead_hours);
    WeatherRecordRow {
        adapterVersion: Nullable::Value("open-meteo-forecast-daily/v4".to_owned()),
        apparentTemperatureC: Nullable::Null,
        blackGlobeTemperatureC: Nullable::Null,
        cloudCoverPercent: Nullable::Null,
        contentHash: format!("{lead_hours:064x}"),
        contractEpoch: Nullable::Value("legacy-v4/fixture".to_owned()),
        deviceModel: Nullable::Null,
        deviceSerial: Nullable::Null,
        deviceVendor: Nullable::Null,
        firstReceivedAt: wire_time(RECEIVED_AT),
        id: format!("forecast-record-{lead_hours}"),
        lastReceivedAt: wire_time(RECEIVED_AT),
        pm25MicrogramsPerCubicMeter: Nullable::Null,
        precipitationMm: Nullable::Null,
        precipitationRateMmPerHour: Nullable::Null,
        pressureHpa: Nullable::Null,
        productRunAt: Nullable::Value(wire_time(REFERENCE_AT)),
        providerKey: "open-meteo".to_owned(),
        providerMetadata: Nullable::Value(vec![entry(
            "dataset",
            JsonValue::String(text("best_match")),
        )]),
        qualityMetadata: Nullable::Null,
        relativeHumidityPercent: Nullable::Null,
        revisionCount: lead_hours - 1,
        siteSlug: "ballydidean".to_owned(),
        sourceConfigFingerprint: Nullable::Value("d".repeat(64)),
        sourceId: "12".to_owned(),
        sourceKey: "open-meteo-forecast-v4".to_owned(),
        sourceKind: SourceKind::Forecast,
        stationSlug: "open-meteo-virtual".to_owned(),
        soilElectricalConductivityMicrosiemensPerCm: Nullable::Null,
        soilMoisturePercent: Nullable::Null,
        solarRadiationWm2: Nullable::Null,
        temperatureC: Nullable::Null,
        upstreamModel: Nullable::Value("best_match".to_owned()),
        upstreamTimezone: "GMT".to_owned(),
        uvIndex: Nullable::Null,
        validAt: wire_time(&valid_at),
        waterLevelM: Nullable::Null,
        wetBulbGlobeTemperatureC: Nullable::Null,
        windDirectionDegrees: Nullable::Null,
        windGustMps: if lead_hours.is_multiple_of(24) {
            Nullable::Null
        } else {
            Nullable::Value(2.0 + lead_hours as f64 / 100.0)
        },
        windSpeedMps: Nullable::Value(1.0 + lead_hours as f64 / 100.0),
    }
}

/// Build the admitted clock table for every header and source row instant.
fn fixture_time() -> FixtureTime {
    let mut values = vec![
        admitted_time("2026-08-28T00:00:00.000Z", BASE_EPOCH_MS - 96 * 3_600_000),
        admitted_time(REFERENCE_AT, BASE_EPOCH_MS),
        admitted_time(RECEIVED_AT, BASE_EPOCH_MS + 5 * 60_000),
        admitted_time(ISSUED_AT, BASE_EPOCH_MS + 35 * 60_000),
    ];
    // admit every exact prospective hourly valid clock
    for lead_hours in 1..=168 {
        values.push(admitted_time(
            &valid_at(lead_hours),
            BASE_EPOCH_MS + i64::try_from(lead_hours).expect("fixture lead") * 3_600_000,
        ));
    }
    FixtureTime { values }
}

/// Build one resolver entry from explicit UTC and epoch evidence.
fn admitted_time(value: &str, epoch_milliseconds: i64) -> (Vec<u16>, CanonicalInstant) {
    (text(value), instant(epoch_milliseconds, value))
}

/// Build one admitted canonical instant.
fn instant(epoch_milliseconds: i64, utc: &str) -> CanonicalInstant {
    CanonicalInstant {
        epoch_milliseconds,
        utc: utc.to_owned(),
    }
}

/// Build one exact hourly UTC string for the fixed September fixture.
fn valid_at(lead_hours: u64) -> String {
    let day = 1 + lead_hours / 24;
    let hour = lead_hours % 24;
    format!("2026-09-{day:02}T{hour:02}:00:00.000Z")
}

/// Encode one finite metric exactly as Node big-endian binary64 hexadecimal.
fn binary64_hex(value: f64) -> String {
    format!("{:016x}", value.to_bits())
}

/// Build one generated UTF-16 string.
fn text(value: &str) -> Vec<u16> {
    value.encode_utf16().collect()
}

/// Build one generated string timestamp.
fn wire_time(value: &str) -> JsDateOrString {
    JsDateOrString::String(text(value))
}

/// Build one generated ordered JSON entry.
fn entry(key: &str, value: JsonValue) -> (Vec<u16>, JsonValue) {
    (text(key), value)
}
