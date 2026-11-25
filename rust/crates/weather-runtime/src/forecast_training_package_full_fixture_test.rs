use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::{fs, io};

use weather_contracts::JsonValue;
use weather_database::Sha256Digest;

use super::*;
use crate::MaintenanceCanonicalJson;
use crate::NativeForecastTrainingPackageFiles;
use crate::forecast_training_package_manifest::{
    FORECAST_TRAINING_METRIC_NAMES, FORECAST_TRAINING_STATION_KEYS,
};
use crate::forecast_training_package_row::FORECAST_TRAINING_SOURCE_IDENTITIES;

const FIXTURE_DATE: &str = "2026-10-01";
const FIXTURE_ROOT: &str = "package";
pub(crate) const MANIFEST_DIGEST: &str =
    "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
const MIGRATION_HISTORY_DIGEST: &str =
    "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const MANIFEST_BYTES: &[u8] = b"canonical forecast training fixture manifest\n";
const FIXTURE_INSTANTS: [&str; 24] = [
    "2026-10-01T07:00:00.000Z",
    "2026-10-01T08:00:00.000Z",
    "2026-10-01T09:00:00.000Z",
    "2026-10-01T10:00:00.000Z",
    "2026-10-01T11:00:00.000Z",
    "2026-10-01T12:00:00.000Z",
    "2026-10-01T13:00:00.000Z",
    "2026-10-01T14:00:00.000Z",
    "2026-10-01T15:00:00.000Z",
    "2026-10-01T16:00:00.000Z",
    "2026-10-01T17:00:00.000Z",
    "2026-10-01T18:00:00.000Z",
    "2026-10-01T19:00:00.000Z",
    "2026-10-01T20:00:00.000Z",
    "2026-10-01T21:00:00.000Z",
    "2026-10-01T22:00:00.000Z",
    "2026-10-01T23:00:00.000Z",
    "2026-10-02T00:00:00.000Z",
    "2026-10-02T01:00:00.000Z",
    "2026-10-02T02:00:00.000Z",
    "2026-10-02T03:00:00.000Z",
    "2026-10-02T04:00:00.000Z",
    "2026-10-02T05:00:00.000Z",
    "2026-10-02T06:00:00.000Z",
];
const FIXED_HASHES: [(&str, &str, &str); 8] = [
    (
        "aggregationContractSha256",
        "aggregation_contract_sha256",
        "9c309ef5a00780167570746ad6c31b9128c266db50954fe4645287e1f2b31e64",
    ),
    (
        "coordinateManifestSha256",
        "coordinate_manifest_sha256",
        "04bfd93a03c393e977c8767a9aca6fe2a4cba9c263cb46e6987fa733b666ba58",
    ),
    (
        "metricEligibilitySha256",
        "metric_eligibility_sha256",
        "53731954b347836a26500b05a195ca15cf26214c4d561fe482c5ff87ef56a82e",
    ),
    (
        "queryContractSha256",
        "query_contract_sha256",
        "3b7926c47bbdb208ac2e305ee7798bfe4ea9590ce2863f556e752a71d1158e76",
    ),
    (
        "rowSchemaSha256",
        "row_schema_sha256",
        "2717b6c3c704a1b52c7748b59c37d635efd92d92efb9dc97ea4ddef97cd504fc",
    ),
    (
        "sourceLineageSha256",
        "source_lineage_sha256",
        "261a134589a12c1bbbd9a783343950317fd1fbc87e08383e60e805b7761566cc",
    ),
    (
        "spatialWeightsSha256",
        "spatial_weights_sha256",
        "8ed5ce70d33edd4a5166049d9938cbaaf800151b6a0b3345d3005419e9041c74",
    ),
    (
        "stationManifestSha256",
        "station_manifest_sha256",
        "a1f76440c056987bbb434d5315e4916f961deeb2951fe889d785943f559cdd49",
    ),
];
const MIGRATIONS: [(&str, &str); 13] = [
    (
        "0001_initial_weather.sql",
        "f4264482606a3476b4b54e3089e66c900f0ad5104ea678a9bf17e5e5ba385a62",
    ),
    (
        "0002_worker_migration_readiness.sql",
        "954731c9ddb791c85defc3bffd887a3d30e9ff9fb5c9dac96e050ac52a796f03",
    ),
    (
        "0003_ecowitt_measurements.sql",
        "250e9b49176fda732b23f797a6dbce6878ca93818fa1bf6a408b5d6611ee5a48",
    ),
    (
        "0004_tempest_metadata.sql",
        "5487c90a4e70f8e049f9ed48c3ee3f025eb9f1908ed04a1febfb736c22f30d34",
    ),
    (
        "0005_source_supersession.sql",
        "462be10d1a35419b52068d09ae5254134e0168fed0ada66f957c87d290c8e6f1",
    ),
    (
        "0006_station_coordinates.sql",
        "33f3fc5b1b475add0e862b45488a4a3482435df0ae674599005bafd37c3886a8",
    ),
    (
        "0007_tide_sources.sql",
        "4e2ed3037ea82011e947e48102149a6ea6456e5fb9f504436f7ec35f6e191390",
    ),
    (
        "0008_ecowitt_property_sensors.sql",
        "3934568dcdbb840f5026a09bab676bede7ad7d2571c1d7415efdabb902dd8b81",
    ),
    (
        "0009_forecast_anchor_records.sql",
        "e144415b6ac8e19338cc4b53d7daeffbf3d32650af880ab7acb809c6f71ded20",
    ),
    (
        "0010_forecast_training_export.sql",
        "2590423b7787b0e5e5668bfa4af21db775b8926540b1d2e339d6656aa3f27dc8",
    ),
    (
        "0011_forecast_runtime_provenance.sql",
        "ebf5789636095982e6010a868cdfdd4d7449ca79fb1057adf3b8dd8f49a337dc",
    ),
    (
        "0012_hide_archive_only_forecasts_from_live_reads.sql",
        "663ed323104bb70f4d0e735c3b546fb111038be4d0fee83e0c76c018cf0da15c",
    ),
    (
        "0013_ecmwf_temperature_canary.sql",
        "e05f0397529108641ad9b9eb5381ac968f35449514fe4d8ed0ac502c9d5bf3ee",
    ),
];

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

pub(crate) struct FixtureDirectory(PathBuf);

impl FixtureDirectory {
    /// create a process-owned directory for the full package
    pub(crate) fn new() -> Self {
        let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "weather-forecast-package-full-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&root).expect("fresh fixture directory");
        Self(root)
    }

    /// return the extracted package root on the same filesystem as the fixture
    pub(crate) fn package_path(&self) -> PathBuf {
        fs::canonicalize(&self.0)
            .expect("canonical fixture root")
            .join(FIXTURE_ROOT)
    }
}

impl Drop for FixtureDirectory {
    /// remove only the files created by this fixture
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[derive(Clone)]
pub(crate) struct FixtureValues {
    decoded_rows: BTreeMap<Vec<u8>, JsonValue>,
    digests: BTreeMap<Vec<u8>, String>,
    files: NativeForecastTrainingPackageFiles,
    inflated: BTreeMap<Vec<u8>, Vec<u8>>,
    manifest: JsonValue,
    effects: Rc<RefCell<Vec<&'static str>>>,
}

impl FixtureValues {
    /// build one exact one-day package with eleven complete station partitions
    pub(crate) fn complete_day(directory: &FixtureDirectory) -> Self {
        let mut decoded_rows = BTreeMap::new();
        let mut digests = BTreeMap::new();
        let mut files = BTreeMap::new();
        let mut inflated = BTreeMap::new();
        let mut members = Vec::new();

        // build every station member from twenty-four independently decoded rows
        for (member_index, (station_key, _)) in FORECAST_TRAINING_STATION_KEYS.iter().enumerate() {
            let member_path = format!("members/{FIXTURE_DATE}/station-hour/{station_key}.jsonl.gz");
            let compressed = format!("synthetic-gzip-member:{station_key}").into_bytes();
            let member_digest = format!("{:064x}", member_index + 1);
            let mut lines = Vec::with_capacity(FIXTURE_INSTANTS.len());

            // retain one exact token and typed decoded row for each hourly line
            for hour in 0..FIXTURE_INSTANTS.len() {
                let line = format!("{station_key}|{hour:02}");
                decoded_rows.insert(line.as_bytes().to_vec(), station_gap_row(station_key, hour));
                lines.push(line);
            }

            let plaintext = format!("{}\n", lines.join("\n")).into_bytes();
            members.push(member(
                station_key,
                &member_path,
                &member_digest,
                compressed.len(),
                plaintext.len(),
            ));
            digests.insert(compressed.clone(), member_digest);
            files.insert(member_path, compressed.clone());
            inflated.insert(compressed, plaintext);
        }

        let manifest = manifest(members);
        files.insert(MANIFEST_PATH.to_owned(), MANIFEST_BYTES.to_vec());
        files.insert(
            MANIFEST_CHECKSUM_PATH.to_owned(),
            format!("{MANIFEST_DIGEST}  {MANIFEST_PATH}\n").into_bytes(),
        );
        digests.insert(MANIFEST_BYTES.to_vec(), MANIFEST_DIGEST.to_owned());
        digests.insert(migration_preimage(), MIGRATION_HISTORY_DIGEST.to_owned());
        let working_directory =
            fs::canonicalize(&directory.0).expect("canonical fixture working directory");
        let package = working_directory.join(FIXTURE_ROOT);
        fs::create_dir_all(package.join(format!("members/{FIXTURE_DATE}/station-hour")))
            .expect("forecast fixture partitions");
        // exercise the native owner against real control and member files
        for (path, bytes) in files {
            fs::write(package.join(path), bytes).expect("forecast fixture file");
        }

        Self {
            decoded_rows,
            digests,
            files: NativeForecastTrainingPackageFiles::new(working_directory)
                .expect("bound native forecast filesystem"),
            inflated,
            manifest,
            effects: Rc::default(),
        }
    }

    /// return the shared snapshot and verifier order from the fixture
    pub(crate) fn effects(&self) -> Vec<&'static str> {
        self.effects.borrow().clone()
    }
}

impl crate::RainPerformanceTimestampValues for FixtureValues {
    /// parse only the row or manifest instants covered by the complete day
    fn parse_js_timestamp_ms(&mut self, value: &[u16]) -> Option<i64> {
        let value = String::from_utf16(value).ok()?;
        // preserve the preceding source run and receipt clocks used by temperature data
        match value.as_str() {
            "2026-10-01T00:00:00.000Z" => Some(-25_200_000),
            "2026-10-01T06:03:00.000Z" => Some(-3_420_000),
            _ => ForecastTrainingPackageManifestValues::utc_epoch_milliseconds(self, &value),
        }
    }

    /// spell the exact available UTC instant used during rain package loading
    fn format_utc_milliseconds(&mut self, epoch_ms: i64) -> Option<Vec<u16>> {
        // admit the end-of-day manifest creation timestamp separately
        match epoch_ms {
            -25_200_000 => Some("2026-10-01T00:00:00.000Z".encode_utf16().collect()),
            -3_420_000 => Some("2026-10-01T06:03:00.000Z".encode_utf16().collect()),
            86_400_000 => Some("2026-10-02T07:00:00.000Z".encode_utf16().collect()),
            _ => self
                .canonical_utc(epoch_ms)
                .map(|value| value.encode_utf16().collect()),
        }
    }
}

impl crate::RainPerformanceEdgeRowsValues for FixtureValues {
    /// decode the one forecast manifest or its independent simulated row tokens
    fn decode_json(&mut self, bytes: &[u8]) -> Option<JsonValue> {
        // distinguish the control file from the row stream's exact tokens
        if bytes == MANIFEST_BYTES {
            Some(self.manifest.clone())
        } else {
            self.decoded_rows.get(bytes).cloned()
        }
    }

    /// keep edge decoding unused for the empty adjustment freeze
    fn gunzip_bounded(&mut self, _: &[u8], _: usize) -> Option<Vec<u8>> {
        unreachable!("empty adjustment edge freeze")
    }
}

impl crate::RainPerformancePackageLoaderValues for FixtureValues {
    type Error = io::Error;

    /// resolve both actual fixture package paths before verification
    fn resolve_package_root(&mut self, raw: &str) -> io::Result<String> {
        self.files.absolute_path(raw)
    }

    /// read the forecast path from its actual opened regular member
    fn read_package_file(&mut self, root: &str, member: &str) -> io::Result<Vec<u8>> {
        // observe the command's pre-verification control read
        if member == MANIFEST_PATH {
            self.effects.borrow_mut().push("snapshot");
        }
        self.files.read_regular_file(root, member)?.ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "expected forecast member fixture")
        })
    }

    /// return the exact fixture rows for the verified compressed member
    fn gunzip_forecast_member(&mut self, bytes: &[u8]) -> io::Result<Vec<u8>> {
        self.inflated
            .get(bytes)
            .cloned()
            .ok_or_else(|| io::Error::other("unexpected fixture forecast member"))
    }
}

impl MaintenanceCanonicalJson for FixtureValues {
    /// parse only the exact immutable manifest byte population
    fn parse_canonical_object(&mut self, bytes: &[u8]) -> Option<JsonValue> {
        (bytes == MANIFEST_BYTES).then(|| self.manifest.clone())
    }

    /// reencode only the exact decoded fixture manifest to its original bytes
    fn encode_canonical_json(&mut self, value: &JsonValue) -> Option<Vec<u8>> {
        (value == &self.manifest).then(|| MANIFEST_BYTES.to_vec())
    }

    /// return the exact digest bound to each immutable fixture byte population
    fn sha256(&mut self, bytes: &[u8]) -> Option<Sha256Digest> {
        self.digests.get(bytes).cloned().map(Sha256Digest::admitted)
    }
}

impl ForecastTrainingPackageManifestValues for FixtureValues {
    /// admit only the two real calendar dates present in the fixture instants
    fn local_date_ordinal(&mut self, date: &str) -> Option<i64> {
        // keep the package date and UTC rollover date consecutive
        match date {
            "2026-10-01" => Some(0),
            "2026-10-02" => Some(1),
            _ => None,
        }
    }

    /// parse the hourly fixture instants plus the package creation instant
    fn utc_epoch_milliseconds(&mut self, instant: &str) -> Option<i64> {
        fixture_epoch(instant).or_else(|| (instant == "2026-10-02T07:00:00Z").then_some(86_400_000))
    }
}

impl ForecastTrainingPackageRowTime for FixtureValues {
    /// parse only the exact canonical hourly row instants
    fn utc_epoch_milliseconds(&self, instant: &str) -> Option<i64> {
        fixture_epoch(instant)
    }

    /// format only the exact hourly epochs back to canonical millisecond instants
    fn canonical_utc(&self, epoch_ms: i64) -> Option<String> {
        let hour = usize::try_from(epoch_ms.checked_div(3_600_000)?).ok()?;
        FIXTURE_INSTANTS.get(hour).map(ToString::to_string)
    }

    /// project every admitted row epoch into the one fixture local date
    fn los_angeles_local_date(&self, epoch_ms: i64) -> Option<String> {
        let present = usize::try_from(epoch_ms.checked_div(3_600_000)?)
            .ok()
            .is_some_and(|hour| hour < FIXTURE_INSTANTS.len());
        present.then(|| FIXTURE_DATE.to_owned())
    }
}

impl ForecastTrainingPackageVerificationValues for FixtureValues {
    type Error = io::Error;

    /// resolve the caller's package path with native file operations
    fn absolute_path(&mut self, raw: &str) -> Result<String, Self::Error> {
        self.effects.borrow_mut().push("verify");
        self.files.absolute_path(raw)
    }

    /// return the exact real path for the fixture root
    fn canonical_package_path(&mut self, absolute: &str) -> Result<Option<String>, Self::Error> {
        self.files.canonical_package_path(absolute)
    }

    /// read one exact control or declared member byte population
    fn read_regular_file(
        &mut self,
        root: &str,
        relative_path: &str,
    ) -> Result<Option<Vec<u8>>, Self::Error> {
        self.files.read_regular_file(root, relative_path)
    }

    /// return the exact regular-file and real-directory inventory
    fn collect_regular_tree_entries(
        &mut self,
        root: &str,
    ) -> Result<Option<Vec<String>>, Self::Error> {
        self.files.collect_regular_tree_entries(root)
    }

    /// decode only one of the 264 exact synthetic JSONL row tokens
    fn decode_json(&mut self, bytes: &[u8]) -> Option<JsonValue> {
        self.decoded_rows.get(bytes).cloned()
    }

    /// inflate only an exact declared compressed member population
    fn gunzip(&mut self, compressed: &[u8]) -> Result<Vec<u8>, Self::Error> {
        self.inflated
            .get(compressed)
            .cloned()
            .ok_or_else(|| io::Error::other("unexpected compressed member"))
    }

    /// enumerate the fixture's sole inclusive local date
    fn inclusive_local_dates(&mut self, from: &str, to: &str) -> Option<Vec<String>> {
        ((from, to) == (FIXTURE_DATE, FIXTURE_DATE)).then(|| vec![FIXTURE_DATE.to_owned()])
    }

    /// return the exact UTC hourly instants belonging to the fixture local date
    fn local_date_utc_hours(&mut self, local_date: &str) -> Option<Vec<i64>> {
        (local_date == FIXTURE_DATE).then(|| {
            (0..FIXTURE_INSTANTS.len())
                .map(|hour| i64::try_from(hour).expect("fixture hour fits") * 3_600_000)
                .collect()
        })
    }
}

/// verify one complete package through the public top-level verifier boundary
#[test]
fn verifies_complete_eleven_station_day_and_returns_manifest_digest() {
    let directory = FixtureDirectory::new();
    let mut values = FixtureValues::complete_day(&directory);
    let digest = verify_forecast_training_package(FIXTURE_ROOT, &mut values)
        .expect("complete forecast package verified");
    assert_eq!(digest, MANIFEST_DIGEST);
    assert_eq!(
        verify_forecast_training_package("package/manifest.json", &mut values),
        Err(ForecastTrainingPackageVerificationError::ControlFiles)
    );
}

/// build the exact official manifest with one complete day of declared members
fn manifest(members: Vec<JsonValue>) -> JsonValue {
    let database_manifest = database_manifest();
    let mut fields = FIXED_HASHES
        .iter()
        .map(|(package_key, _, digest)| (*package_key, string(digest)))
        .collect::<Vec<_>>();
    fields.extend([
        (
            "contractVersion",
            string("forecast-training-export-package/v1"),
        ),
        ("createdAtUtc", string("2026-10-02T07:00:00Z")),
        ("databaseManifest", database_manifest),
        ("fromLocalDate", string(FIXTURE_DATE)),
        ("limits", limits()),
        ("members", JsonValue::Array(members)),
        ("migrationHistorySha256", string(MIGRATION_HISTORY_DIGEST)),
        ("observedSourceIdentities", JsonValue::Array(Vec::new())),
        (
            "queryContractVersion",
            string("forecast-training-export-query/v2"),
        ),
        ("siteKey", string("ballydidean")),
        ("siteTimezone", string("America/Los_Angeles")),
        ("sourceIdentities", source_identities()),
        ("stationMetricCoverage", station_coverage()),
        ("toLocalDate", string(FIXTURE_DATE)),
        ("totalRowCount", number(264)),
        ("transaction", transaction()),
        ("usageBoundary", usage_boundary()),
    ]);
    object(fields)
}

/// build the frozen upstream manifest and reviewed thirteen-migration prefix
fn database_manifest() -> JsonValue {
    let mut fields = FIXED_HASHES
        .iter()
        .map(|(_, database_key, digest)| (*database_key, string(digest)))
        .collect::<Vec<_>>();
    fields.extend([
        (
            "contract_version",
            string("forecast-training-export-manifest/v1"),
        ),
        (
            "migration_checksums",
            string_array(MIGRATIONS.map(|entry| entry.1)),
        ),
        ("migration_history_sha256", string(MIGRATION_HISTORY_DIGEST)),
        (
            "migration_names",
            string_array(MIGRATIONS.map(|entry| entry.0)),
        ),
        (
            "query_contract_version",
            string("forecast-training-export-query/v2"),
        ),
        (
            "schema_migration",
            string("0010_forecast_training_export.sql"),
        ),
        ("site_key", string("ballydidean")),
        ("site_timezone", string("America/Los_Angeles")),
    ]);
    object(fields)
}

/// build one member declaration bound to its exact compressed and plaintext bytes
fn member(
    station_key: &str,
    path: &str,
    digest: &str,
    size_bytes: usize,
    plaintext_bytes: usize,
) -> JsonValue {
    object(vec![
        ("localDate", string(FIXTURE_DATE)),
        ("maxValidAt", string(FIXTURE_INSTANTS[23])),
        ("minValidAt", string(FIXTURE_INSTANTS[0])),
        ("path", string(path)),
        (
            "plaintextBytes",
            number(u64::try_from(plaintext_bytes).expect("fixture plaintext fits")),
        ),
        ("recordKind", string("station-hour")),
        ("rowCount", number(24)),
        ("sha256", string(digest)),
        (
            "sizeBytes",
            number(u64::try_from(size_bytes).expect("fixture member fits")),
        ),
        ("stationKey", string(station_key)),
    ])
}

/// build one explicit provenance-free gap row for a station and exact hour
fn station_gap_row(station_key: &str, hour: usize) -> JsonValue {
    object(vec![
        ("adapter_contracts", JsonValue::Array(Vec::new())),
        ("collision_count", number(0)),
        ("content_hashes", JsonValue::Array(Vec::new())),
        ("contract_epoch", string("physical-station-hourly/v1")),
        ("dataset", JsonValue::Null),
        (
            "exclusion_reason_codes",
            JsonValue::Array(vec![string("station_coverage_insufficient")]),
        ),
        ("ingestion_run_ids", JsonValue::Array(Vec::new())),
        ("physical_station_key", string(station_key)),
        ("provider_family", string(provider_family(station_key))),
        ("record_kind", string("station_hour")),
        ("received_at", JsonValue::Null),
        ("reference_at", JsonValue::Null),
        ("reference_kind", JsonValue::Null),
        ("relative_humidity_percent", JsonValue::Null),
        ("site_key", string("ballydidean")),
        ("source_config_fingerprints", JsonValue::Array(Vec::new())),
        ("source_keys", JsonValue::Array(Vec::new())),
        ("target_lead_hours", JsonValue::Null),
        ("temperature_c", JsonValue::Null),
        ("upstream_model", JsonValue::Null),
        ("valid_at", string(FIXTURE_INSTANTS[hour])),
        ("wind_direction_degrees", JsonValue::Null),
        ("wind_gust_mps", JsonValue::Null),
        ("wind_speed_mps", JsonValue::Null),
    ])
}

/// return the frozen provider owner for one generated station key
fn provider_family(station_key: &str) -> &'static str {
    // mirror the closed provider projection required by row admission
    match station_key {
        "ambient-maxweather" | "ambient-merlin" => "ambient",
        "ballydidean-ecowitt" => "ecowitt",
        "netatmo-nearby" => "netatmo",
        "tempest-126537" | "tempest-168853" | "tempest-201058" | "tempest-203055"
        | "tempest-225947" | "tempest-38270" | "tempest-64255" => "tempest",
        _ => panic!("fixture station roster drifted"),
    }
}

/// build the complete immutable source roster in its canonical order
fn source_identities() -> JsonValue {
    JsonValue::Array(
        FORECAST_TRAINING_SOURCE_IDENTITIES
            .iter()
            .map(|identity| {
                object(vec![
                    ("adapterContract", string(identity.adapter_contract)),
                    (
                        "sourceConfigFingerprint",
                        string(identity.source_config_fingerprint),
                    ),
                    ("sourceKey", string(identity.source_key)),
                ])
            })
            .collect(),
    )
}

/// build zero non-null metric coverage for every all-gap station member
fn station_coverage() -> JsonValue {
    JsonValue::Array(
        FORECAST_TRAINING_STATION_KEYS
            .iter()
            .map(|(station_key, _)| {
                object(vec![
                    (
                        "eligibleMetricNonNullLocalDates",
                        object(
                            FORECAST_TRAINING_METRIC_NAMES
                                .iter()
                                .map(|metric| (*metric, number(0)))
                                .collect(),
                        ),
                    ),
                    ("stationKey", string(station_key)),
                ])
            })
            .collect(),
    )
}

/// build the immutable export limit declaration
fn limits() -> JsonValue {
    object(vec![
        (
            "conservativeExportRowFormula",
            string("450 * ((24 * 264) + (11 * 24) + 168)"),
        ),
        ("conservativeExportRows", number(3_045_600)),
        ("exportRowHeadroom", number(954_400)),
        ("maxDays", number(450)),
        ("maxRows", number(4_000_000)),
        ("rowCountMeaning", string("export_rows_not_training_events")),
    ])
}

/// build the immutable transaction declaration
fn transaction() -> JsonValue {
    object(vec![
        ("idleInTransactionSessionTimeout", string("30s")),
        ("isolationLevel", string("repeatable read")),
        ("lockTimeout", string("5s")),
        ("readOnly", string("on")),
        ("statementTimeout", string("15min")),
    ])
}

/// build the immutable snapshot-only usage declaration
fn usage_boundary() -> JsonValue {
    object(vec![
        ("databaseImportAllowed", JsonValue::Boolean(false)),
        ("productionDerived", JsonValue::Boolean(true)),
        ("snapshotOnly", JsonValue::Boolean(true)),
    ])
}

/// reproduce the exact reviewed migration-history digest preimage
fn migration_preimage() -> Vec<u8> {
    MIGRATIONS
        .iter()
        .map(|(name, checksum)| format!("{name}:{checksum}"))
        .collect::<Vec<_>>()
        .join("\n")
        .into_bytes()
}

/// parse one exact hourly row instant into its synthetic monotonic epoch
fn fixture_epoch(instant: &str) -> Option<i64> {
    FIXTURE_INSTANTS
        .iter()
        .position(|candidate| *candidate == instant)
        .and_then(|hour| i64::try_from(hour).ok())
        .map(|hour| hour * 3_600_000)
}

/// build one dependency-free JSON object from UTF-8 fixture keys
fn object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(key, value)| (key.encode_utf16().collect(), value))
            .collect(),
    )
}

/// build one dependency-free JSON string
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// build one dependency-free JSON integer within the fixture's safe range
fn number(value: u64) -> JsonValue {
    JsonValue::Number(value as f64)
}

/// build one dependency-free JSON string array
fn string_array<const N: usize>(values: [&str; N]) -> JsonValue {
    JsonValue::Array(values.into_iter().map(string).collect())
}
