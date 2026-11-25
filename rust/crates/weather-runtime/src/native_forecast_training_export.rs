//! native producer for the predecessor forecast-training package archive.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use flate2::read::MultiGzDecoder;
use jiff::{Span, Timestamp, civil::Date, tz::TimeZone};
use weather_contracts::JsonValue;
use weather_database::Sha256Digest;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};

use crate::forecast_training_package_manifest::{
    FORECAST_TRAINING_METRIC_NAMES, FORECAST_TRAINING_STATION_KEYS,
    ForecastTrainingPackageManifestValues, ForecastTrainingPackageSourceIdentity,
    admit_forecast_training_package_manifest,
};
use crate::forecast_training_package_row::{
    FORECAST_TRAINING_SOURCE_IDENTITIES, ForecastTrainingPackageRowTime,
    ForecastTrainingRecordKind, ForecastTrainingRowMetrics, validate_forecast_training_package_row,
};
use crate::forecast_training_package_verification::{
    ForecastTrainingPackageVerificationValues, verify_forecast_training_package,
};
use crate::maintenance_canonical_json::{exact_keys, field, text};
use crate::native_open_meteo_values::load_named_timezone;
use crate::{MaintenanceCanonicalJson, NativeForecastTrainingPackageFiles, NativeWirePrimitives};

const MAXIMUM_ROWS: u64 = 4_000_000;
const TAR_BLOCK: usize = 512;
const TAR_RECORD: usize = 20 * TAR_BLOCK;
const TRANSACTION_KEYS: &[&str] = &[
    "created_at_utc",
    "from_local_date",
    "idle_in_transaction_session_timeout",
    "isolation_level",
    "lock_timeout",
    "read_only",
    "statement_timeout",
    "to_local_date",
];

/// exact identities returned after one complete native export stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeForecastTrainingExportReceipt {
    pub manifest_sha256: String,
    pub row_count: u64,
}

impl NativeForecastTrainingExportReceipt {
    /// render the predecessor shell's one-line completion report.
    pub fn completion_report(&self, from: &str, to: &str) -> String {
        format!(
            "Streaming verified Ballydidean forecast-training export {from}..{to} manifest={} rows<=4000000 oracle=3045600 export_rows_not_training_events\n",
            self.manifest_sha256
        )
    }
}

/// stream one PostgreSQL JSONL export into the exact deterministic package tar.gz.
pub fn stream_native_forecast_training_export(
    mut input: impl BufRead,
    from: &str,
    to: &str,
    scratch_root: &Path,
    output: impl Write,
) -> io::Result<NativeForecastTrainingExportReceipt> {
    let mut values = NativeForecastTrainingExportValues::new()?;
    let from_ordinal = values
        .local_date_ordinal(from)
        .ok_or_else(|| invalid("export from date differs"))?;
    let to_ordinal = values
        .local_date_ordinal(to)
        .ok_or_else(|| invalid("export to date differs"))?;
    // retain the original inclusive date ceiling
    if !(1..=450).contains(&(to_ordinal - from_ordinal + 1)) {
        return Err(invalid("export date range differs"));
    }

    let scratch = ScratchDirectory::create(scratch_root)?;
    let mut spool = MemberSpool::new(scratch.path());
    let mut members = BTreeMap::<String, PendingMember>::new();
    let mut coverage = StationCoverage::new();
    let mut observed = BTreeMap::<String, ForecastTrainingPackageSourceIdentity>::new();
    let mut database_manifest = None;
    let mut transaction = None;
    let mut row_count = 0_u64;

    // consume the same leading manifest and subsequent row grammar
    loop {
        let mut line = Vec::new();
        let read = input.read_until(b'\n', &mut line)?;
        // stop only at the real stream boundary
        if read == 0 {
            break;
        }
        // remove the readline delimiter
        if line.last() == Some(&b'\n') {
            line.pop();
        }
        // reproduce crlf delay normalization
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        // reject empty source records
        if line.is_empty() {
            return Err(invalid("transaction stream contains a blank line"));
        }
        let envelope = NativeWirePrimitives
            .decode_json(&line)
            .ok_or_else(|| invalid("transaction record differs"))?;
        let kind = text(&envelope, "record_type")
            .ok_or_else(|| invalid("transaction record type differs"))?;
        // accept exactly one leading manifest
        if kind == "manifest" {
            // reject duplicate or late manifests
            if database_manifest.is_some()
                || row_count != 0
                || !exact_keys(&envelope, &["payload", "record_type", "transaction"])
            {
                return Err(invalid("transaction manifest position differs"));
            }
            database_manifest = Some(
                field(&envelope, "payload")
                    .ok_or_else(|| invalid("database manifest unavailable"))?
                    .clone(),
            );
            transaction = Some(validate_transaction(
                &values,
                field(&envelope, "transaction")
                    .ok_or_else(|| invalid("transaction evidence unavailable"))?,
                from,
                to,
            )?);
            continue;
        }
        // reject rows before the manifest or with an open shape
        if kind != "row"
            || database_manifest.is_none()
            || !exact_keys(&envelope, &["payload", "record_type"])
        {
            return Err(invalid("transaction row ordering differs"));
        }
        row_count = row_count
            .checked_add(1)
            .filter(|count| *count <= MAXIMUM_ROWS)
            .ok_or_else(|| invalid("export row count exceeds 4000000"))?;
        let row = field(&envelope, "payload")
            .ok_or_else(|| invalid("export row unavailable"))?
            .clone();
        let admitted = validate_forecast_training_package_row(&values, row, from, to)
            .map_err(io::Error::other)?;
        let canonical = NativeWirePrimitives
            .encode_sorted_json(&admitted.row)
            .ok_or_else(|| invalid("export row canonical encoding differs"))?;
        coverage.observe(
            admitted.record_kind,
            admitted.station_key.as_deref(),
            &admitted.local_date,
            admitted.metrics,
        )?;
        // retain each aligned source identity once by source key
        for &identity in &admitted.source_identities {
            // reject only conflicting repetitions
            if observed
                .insert(identity.source_key.to_owned(), identity)
                .is_some_and(|previous| previous != identity)
            {
                return Err(invalid("observed source identity is duplicated"));
            }
        }
        let member = members
            .entry(admitted.member_path.clone())
            .or_insert_with(|| PendingMember::new(&admitted));
        member.observe(&admitted, canonical.len())?;
        spool.append(&admitted.member_path, &canonical)?;
    }
    drop(spool);

    let database_manifest = database_manifest
        .ok_or_else(|| invalid("transaction stream is missing its database manifest"))?;
    let transaction = transaction
        .ok_or_else(|| invalid("transaction stream is missing its database manifest"))?;
    let mut archive_files = BTreeMap::<String, PathBuf>::new();
    let mut finalized = Vec::with_capacity(members.len());
    // finalize one member at a time to avoid retaining the complete package body
    for (path, member) in members {
        let plaintext_path = spooled_path(scratch.path(), &path)?;
        let mut plaintext = Vec::new();
        fs::File::open(&plaintext_path)?.read_to_end(&mut plaintext)?;
        let plaintext = sort_member_lines(plaintext)?;
        let compressed = crate::native_node_gzip_candidate(&plaintext)
            .ok_or_else(|| invalid("forecast-training member compression failed"))?;
        finalized.push(member.finalize(&path, &compressed)?);
        let compressed_path = scratch.path().join(&path);
        // retain the validated member directory shape
        if let Some(parent) = compressed_path.parent() {
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(parent)?;
        }
        write_private_file(&compressed_path, &compressed)?;
        fs::remove_file(plaintext_path)?;
        archive_files.insert(path, compressed_path);
    }

    let manifest = build_manifest(
        database_manifest,
        transaction,
        from,
        to,
        row_count,
        finalized,
        observed,
        coverage,
    )?;
    let admitted = admit_forecast_training_package_manifest(&mut values, &manifest)
        .ok_or_else(|| invalid("built forecast-training manifest differs"))?;
    // bind the returned typed total
    if admitted.total_row_count != row_count {
        return Err(invalid("built forecast-training row count differs"));
    }
    let manifest_bytes = NativeWirePrimitives
        .encode_sorted_json(&manifest)
        .ok_or_else(|| invalid("forecast-training manifest encoding differs"))?;
    let manifest_sha256 = sha256(&manifest_bytes);
    let manifest_path = scratch.path().join("manifest.json");
    let checksum_path = scratch.path().join("manifest.sha256");
    write_private_file(&manifest_path, &manifest_bytes)?;
    write_private_file(
        &checksum_path,
        format!("{manifest_sha256}  manifest.json\n").as_bytes(),
    )?;
    archive_files.insert("manifest.json".to_owned(), manifest_path);
    archive_files.insert("manifest.sha256".to_owned(), checksum_path);
    let canonical_scratch = fs::canonicalize(scratch.path())?;
    let verified_sha256 = verify_forecast_training_package(
        canonical_scratch
            .to_str()
            .ok_or_else(|| invalid("forecast-training scratch path differs"))?,
        &mut values,
    )
    .map_err(io::Error::other)?;
    // require the complete extracted-package proof to retain the built identity
    if verified_sha256 != manifest_sha256 {
        return Err(invalid("forecast-training package identity changed"));
    }
    write_native_forecast_training_package_archive(archive_files, output)?;
    drop(scratch);
    Ok(NativeForecastTrainingExportReceipt {
        manifest_sha256,
        row_count,
    })
}

#[derive(Clone)]
struct NativeForecastTrainingExportValues {
    files: NativeForecastTrainingPackageFiles,
    timezone: TimeZone,
}

impl NativeForecastTrainingExportValues {
    /// load the one reviewed site timezone before reading export bytes.
    fn new() -> io::Result<Self> {
        Ok(Self {
            files: NativeForecastTrainingPackageFiles::new(std::env::current_dir()?)?,
            timezone: load_named_timezone("America/Los_Angeles")
                .ok_or_else(|| invalid("site timezone unavailable"))?,
        })
    }
}

impl MaintenanceCanonicalJson for NativeForecastTrainingExportValues {
    /// decode only recursively canonical control documents.
    fn parse_canonical_object(&mut self, bytes: &[u8]) -> Option<JsonValue> {
        NativeWirePrimitives.parse_canonical_object(bytes)
    }

    /// encode recursively sorted JSON with one line feed.
    fn encode_canonical_json(&mut self, value: &JsonValue) -> Option<Vec<u8>> {
        NativeWirePrimitives.encode_canonical_json(value)
    }

    /// hash exact package bytes with SHA-256.
    fn sha256(&mut self, bytes: &[u8]) -> Option<Sha256Digest> {
        MaintenanceCanonicalJson::sha256(&mut NativeWirePrimitives, bytes)
    }
}

impl ForecastTrainingPackageManifestValues for NativeForecastTrainingExportValues {
    /// project one canonical UTC date label to a stable ordinal.
    fn local_date_ordinal(&mut self, date: &str) -> Option<i64> {
        date_ordinal(date)
    }

    /// parse the source's explicit UTC instant grammar.
    fn utc_epoch_milliseconds(&mut self, instant: &str) -> Option<i64> {
        NativeWirePrimitives.parse_explicit_utc_milliseconds(instant)
    }
}

impl ForecastTrainingPackageRowTime for NativeForecastTrainingExportValues {
    /// parse one source UTC instant without timezone inference.
    fn utc_epoch_milliseconds(&self, value: &str) -> Option<i64> {
        NativeWirePrimitives.parse_explicit_utc_milliseconds(value)
    }

    /// format one exact millisecond instant in canonical UTC.
    fn canonical_utc(&self, epoch_ms: i64) -> Option<String> {
        NativeWirePrimitives.format_utc_milliseconds(epoch_ms)
    }

    /// resolve the original Ballydidean calendar label.
    fn los_angeles_local_date(&self, epoch_ms: i64) -> Option<String> {
        let timestamp = Timestamp::from_millisecond(epoch_ms).ok()?;
        Some(self.timezone.to_datetime(timestamp).date().to_string())
    }
}

impl ForecastTrainingPackageVerificationValues for NativeForecastTrainingExportValues {
    type Error = io::Error;

    /// resolve one package path from the captured process directory.
    fn absolute_path(&mut self, raw: &str) -> Result<String, Self::Error> {
        self.files.absolute_path(raw)
    }

    /// resolve one nonsymbolic package root to its real identity.
    fn canonical_package_path(&mut self, absolute: &str) -> Result<Option<String>, Self::Error> {
        self.files.canonical_package_path(absolute)
    }

    /// read one declared regular package member without following its leaf.
    fn read_regular_file(
        &mut self,
        root: &str,
        relative_path: &str,
    ) -> Result<Option<Vec<u8>>, Self::Error> {
        self.files.read_regular_file(root, relative_path)
    }

    /// enumerate the exact real directory and regular-file package tree.
    fn collect_regular_tree_entries(
        &mut self,
        root: &str,
    ) -> Result<Option<Vec<String>>, Self::Error> {
        self.files.collect_regular_tree_entries(root)
    }

    /// decode one complete ordinary JSON row.
    fn decode_json(&mut self, bytes: &[u8]) -> Option<JsonValue> {
        NativeWirePrimitives.decode_json(bytes)
    }

    /// inflate one member produced by this process before archive emission.
    fn gunzip(&mut self, compressed: &[u8]) -> Result<Vec<u8>, Self::Error> {
        let mut plaintext = Vec::new();
        MultiGzDecoder::new(compressed).read_to_end(&mut plaintext)?;
        Ok(plaintext)
    }

    /// enumerate exact inclusive UTC calendar labels.
    fn inclusive_local_dates(&mut self, from: &str, to: &str) -> Option<Vec<String>> {
        let mut current = from.parse::<Date>().ok()?;
        let end = to.parse::<Date>().ok()?;
        let one_day = Span::new().try_days(1).ok()?;
        let mut dates = Vec::new();
        // retain each label through the inclusive endpoint
        while current <= end {
            dates.push(current.to_string());
            current = current.checked_add(one_day).ok()?;
        }
        Some(dates)
    }

    /// enumerate the actual twenty-three to twenty-five UTC hours in one site date.
    fn local_date_utc_hours(&mut self, local_date: &str) -> Option<Vec<i64>> {
        let center = date_ordinal(local_date)?.checked_mul(86_400_000)?;
        let start = center.checked_sub(43_200_000)?;
        let end = center.checked_add(129_600_000)?;
        let mut hours = Vec::new();
        let mut current = start;
        // reproduce the predecessor's bounded timezone scan
        while current <= end {
            let timestamp = Timestamp::from_millisecond(current).ok()?;
            // retain only the requested local calendar label
            if self.timezone.to_datetime(timestamp).date().to_string() == local_date {
                hours.push(current);
            }
            current = current.checked_add(3_600_000)?;
        }
        (23..=25).contains(&hours.len()).then_some(hours)
    }
}

struct TransactionEvidence {
    created_at_utc: String,
    idle_timeout: String,
    isolation: String,
    lock_timeout: String,
    read_only: String,
    statement_timeout: String,
}

/// admit and normalize the source transaction witness.
fn validate_transaction(
    values: &NativeForecastTrainingExportValues,
    value: &JsonValue,
    from: &str,
    to: &str,
) -> io::Result<TransactionEvidence> {
    // bind every transaction setting and date
    if !exact_keys(value, TRANSACTION_KEYS)
        || text(value, "read_only").as_deref() != Some("on")
        || text(value, "isolation_level").as_deref() != Some("repeatable read")
        || text(value, "statement_timeout").as_deref() != Some("15min")
        || text(value, "lock_timeout").as_deref() != Some("5s")
        || text(value, "idle_in_transaction_session_timeout").as_deref() != Some("30s")
        || text(value, "from_local_date").as_deref() != Some(from)
        || text(value, "to_local_date").as_deref() != Some(to)
    {
        return Err(invalid("transaction evidence differs"));
    }
    let created = text(value, "created_at_utc")
        .and_then(|instant| {
            values
                .utc_epoch_milliseconds(&instant)
                .map(|epoch| (instant, epoch))
        })
        .ok_or_else(|| invalid("transaction creation instant differs"))?;
    // preserve the source's UTC-only grammar before millisecond normalization
    if !valid_source_utc(&created.0) {
        return Err(invalid("transaction creation instant differs"));
    }
    Ok(TransactionEvidence {
        created_at_utc: values
            .canonical_utc(created.1)
            .ok_or_else(|| invalid("transaction creation instant differs"))?,
        idle_timeout: "30s".to_owned(),
        isolation: "repeatable read".to_owned(),
        lock_timeout: "5s".to_owned(),
        read_only: "on".to_owned(),
        statement_timeout: "15min".to_owned(),
    })
}

/// recognize the source's one-to-six digit UTC fractional grammar.
fn valid_source_utc(value: &str) -> bool {
    let bytes = value.as_bytes();
    (bytes.len() == 20 || (22..=27).contains(&bytes.len()))
        && bytes.get(4) == Some(&b'-')
        && bytes.get(7) == Some(&b'-')
        && bytes.get(10) == Some(&b'T')
        && bytes.get(13) == Some(&b':')
        && bytes.get(16) == Some(&b':')
        && bytes.last() == Some(&b'Z')
        && (bytes.len() == 20 || bytes.get(19) == Some(&b'.'))
        && bytes.iter().enumerate().all(|(index, byte)| {
            matches!(index, 4 | 7 | 10 | 13 | 16)
                || (index == 19 && bytes.len() != 20)
                || index + 1 == bytes.len()
                || byte.is_ascii_digit()
        })
}

struct PendingMember {
    local_date: String,
    maximum_valid_at: String,
    minimum_valid_at: String,
    plaintext_bytes: u64,
    record_kind: ForecastTrainingRecordKind,
    row_count: u64,
    station_key: Option<String>,
}

impl PendingMember {
    /// start one member from its first admitted row identity.
    fn new(
        row: &crate::forecast_training_package_row::ValidatedForecastTrainingPackageRow,
    ) -> Self {
        Self {
            local_date: row.local_date.clone(),
            maximum_valid_at: row.valid_at.clone(),
            minimum_valid_at: row.valid_at.clone(),
            plaintext_bytes: 0,
            record_kind: row.record_kind,
            row_count: 0,
            station_key: row.station_key.clone(),
        }
    }

    /// extend the exact member bounds and source byte count.
    fn observe(
        &mut self,
        row: &crate::forecast_training_package_row::ValidatedForecastTrainingPackageRow,
        bytes: usize,
    ) -> io::Result<()> {
        // retain one stable partition identity
        if self.local_date != row.local_date
            || self.record_kind != row.record_kind
            || self.station_key != row.station_key
        {
            return Err(invalid("member partition identity differs"));
        }
        // retain only the new extrema rather than cloning the existing timestamp on every row
        if row.valid_at < self.minimum_valid_at {
            self.minimum_valid_at.clone_from(&row.valid_at);
        }
        if row.valid_at > self.maximum_valid_at {
            self.maximum_valid_at.clone_from(&row.valid_at);
        }
        self.row_count = self
            .row_count
            .checked_add(1)
            .ok_or_else(|| invalid("member row count differs"))?;
        self.plaintext_bytes = self
            .plaintext_bytes
            .checked_add(u64::try_from(bytes).map_err(io::Error::other)?)
            .ok_or_else(|| invalid("member plaintext size differs"))?;
        Ok(())
    }

    /// bind one compressed member to its final manifest declaration.
    fn finalize(self, path: &str, bytes: &[u8]) -> io::Result<JsonValue> {
        Ok(object(vec![
            ("localDate", string(&self.local_date)),
            ("maxValidAt", string(&self.maximum_valid_at)),
            ("minValidAt", string(&self.minimum_valid_at)),
            ("path", string(path)),
            ("plaintextBytes", number(self.plaintext_bytes)),
            ("recordKind", string(self.record_kind.member_component())),
            ("rowCount", number(self.row_count)),
            ("sha256", string(&sha256(bytes))),
            (
                "sizeBytes",
                number(u64::try_from(bytes.len()).map_err(io::Error::other)?),
            ),
            (
                "stationKey",
                self.station_key.as_deref().map_or(JsonValue::Null, string),
            ),
        ]))
    }
}

type MetricDates = BTreeMap<&'static str, BTreeSet<String>>;

struct StationCoverage(BTreeMap<&'static str, MetricDates>);

impl StationCoverage {
    /// create the complete frozen station and metric matrix.
    fn new() -> Self {
        Self(
            FORECAST_TRAINING_STATION_KEYS
                .iter()
                .map(|(station, _)| {
                    (
                        *station,
                        FORECAST_TRAINING_METRIC_NAMES
                            .iter()
                            .map(|metric| (*metric, BTreeSet::new()))
                            .collect(),
                    )
                })
                .collect(),
        )
    }

    /// count each non-null station metric at most once per local date.
    fn observe(
        &mut self,
        kind: ForecastTrainingRecordKind,
        station: Option<&str>,
        date: &str,
        metrics: ForecastTrainingRowMetrics,
    ) -> io::Result<()> {
        // exclude forecast partitions from station coverage
        if kind != ForecastTrainingRecordKind::StationHour {
            return Ok(());
        }
        let station = station.ok_or_else(|| invalid("station coverage identity differs"))?;
        let counts = self
            .0
            .get_mut(station)
            .ok_or_else(|| invalid("station coverage identity differs"))?;
        let present = [
            metrics.relative_humidity_percent.is_some(),
            metrics.temperature_c.is_some(),
            metrics.wind_direction_degrees.is_some(),
            metrics.wind_gust_mps.is_some(),
            metrics.wind_speed_mps.is_some(),
        ];
        // preserve the frozen metric order
        for (metric, present) in FORECAST_TRAINING_METRIC_NAMES.iter().zip(present) {
            // count only non-null metrics
            if present {
                counts
                    .get_mut(metric)
                    .expect("frozen metric matrix")
                    .insert(date.to_owned());
            }
        }
        Ok(())
    }

    /// serialize coverage in the source's sorted station order.
    fn into_json(self) -> JsonValue {
        JsonValue::Array(
            self.0
                .into_iter()
                .map(|(station, counts)| {
                    object(vec![
                        (
                            "eligibleMetricNonNullLocalDates",
                            object(
                                FORECAST_TRAINING_METRIC_NAMES
                                    .iter()
                                    .map(|metric| {
                                        (
                                            *metric,
                                            number(
                                                u64::try_from(
                                                    counts
                                                        .get(metric)
                                                        .expect("frozen metric")
                                                        .len(),
                                                )
                                                .expect("coverage count fits"),
                                            ),
                                        )
                                    })
                                    .collect(),
                            ),
                        ),
                        ("stationKey", string(station)),
                    ])
                })
                .collect(),
        )
    }
}

/// build the exact recursively sorted package manifest value.
#[allow(clippy::too_many_arguments)]
fn build_manifest(
    database: JsonValue,
    transaction: TransactionEvidence,
    from: &str,
    to: &str,
    row_count: u64,
    members: Vec<JsonValue>,
    observed: BTreeMap<String, ForecastTrainingPackageSourceIdentity>,
    coverage: StationCoverage,
) -> io::Result<JsonValue> {
    let duplicate = |key| {
        field(&database, key)
            .cloned()
            .ok_or_else(|| invalid("database manifest field unavailable"))
    };
    Ok(object(vec![
        (
            "aggregationContractSha256",
            duplicate("aggregation_contract_sha256")?,
        ),
        (
            "contractVersion",
            string("forecast-training-export-package/v1"),
        ),
        (
            "coordinateManifestSha256",
            duplicate("coordinate_manifest_sha256")?,
        ),
        ("createdAtUtc", string(&transaction.created_at_utc)),
        ("databaseManifest", database.clone()),
        ("fromLocalDate", string(from)),
        (
            "limits",
            object(vec![
                (
                    "conservativeExportRowFormula",
                    string("450 * ((24 * 264) + (11 * 24) + 168)"),
                ),
                ("conservativeExportRows", number(3_045_600)),
                ("exportRowHeadroom", number(954_400)),
                ("maxDays", number(450)),
                ("maxRows", number(MAXIMUM_ROWS)),
                ("rowCountMeaning", string("export_rows_not_training_events")),
            ]),
        ),
        ("members", JsonValue::Array(members)),
        (
            "metricEligibilitySha256",
            duplicate("metric_eligibility_sha256")?,
        ),
        (
            "migrationHistorySha256",
            duplicate("migration_history_sha256")?,
        ),
        (
            "observedSourceIdentities",
            JsonValue::Array(observed.into_values().map(source_identity).collect()),
        ),
        ("queryContractSha256", duplicate("query_contract_sha256")?),
        ("queryContractVersion", duplicate("query_contract_version")?),
        ("rowSchemaSha256", duplicate("row_schema_sha256")?),
        ("siteKey", string("ballydidean")),
        ("siteTimezone", string("America/Los_Angeles")),
        (
            "sourceIdentities",
            JsonValue::Array(
                FORECAST_TRAINING_SOURCE_IDENTITIES
                    .iter()
                    .copied()
                    .map(source_identity)
                    .collect(),
            ),
        ),
        ("sourceLineageSha256", duplicate("source_lineage_sha256")?),
        ("spatialWeightsSha256", duplicate("spatial_weights_sha256")?),
        ("stationMetricCoverage", coverage.into_json()),
        (
            "stationManifestSha256",
            duplicate("station_manifest_sha256")?,
        ),
        ("toLocalDate", string(to)),
        ("totalRowCount", number(row_count)),
        (
            "transaction",
            object(vec![
                (
                    "idleInTransactionSessionTimeout",
                    string(&transaction.idle_timeout),
                ),
                ("isolationLevel", string(&transaction.isolation)),
                ("lockTimeout", string(&transaction.lock_timeout)),
                ("readOnly", string(&transaction.read_only)),
                ("statementTimeout", string(&transaction.statement_timeout)),
            ]),
        ),
        (
            "usageBoundary",
            object(vec![
                ("databaseImportAllowed", JsonValue::Boolean(false)),
                ("productionDerived", JsonValue::Boolean(true)),
                ("snapshotOnly", JsonValue::Boolean(true)),
            ]),
        ),
    ]))
}

/// serialize one closed source identity object.
fn source_identity(identity: ForecastTrainingPackageSourceIdentity) -> JsonValue {
    object(vec![
        ("adapterContract", string(identity.adapter_contract)),
        (
            "sourceConfigFingerprint",
            string(identity.source_config_fingerprint),
        ),
        ("sourceKey", string(identity.source_key)),
    ])
}

struct MemberSpool {
    root: PathBuf,
    open: BTreeMap<String, fs::File>,
    order: VecDeque<String>,
}

impl MemberSpool {
    /// create the source's thirty-two descriptor member cache.
    fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            open: BTreeMap::new(),
            order: VecDeque::new(),
        }
    }

    /// append one canonical row with insertion-order descriptor eviction.
    fn append(&mut self, member: &str, bytes: &[u8]) -> io::Result<()> {
        // open a missing member through the bounded cache
        if !self.open.contains_key(member) {
            // evict the source's oldest opened descriptor
            if self.open.len() >= 32 {
                let oldest = self
                    .order
                    .pop_front()
                    .ok_or_else(|| invalid("member descriptor order differs"))?;
                self.open.remove(&oldest);
            }
            let path = spooled_path(&self.root, member)?;
            // create only validated derived parents
            if let Some(parent) = path.parent() {
                fs::DirBuilder::new()
                    .recursive(true)
                    .mode(0o700)
                    .create(parent)?;
            }
            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .mode(0o600)
                .open(path)?;
            self.open.insert(member.to_owned(), file);
            self.order.push_back(member.to_owned());
        }
        self.open
            .get_mut(member)
            .ok_or_else(|| invalid("member descriptor unavailable"))?
            .write_all(bytes)
    }
}

/// create one private regular file without replacing an existing node.
fn write_private_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(path)?
        .write_all(bytes)
}

/// resolve only the validated member path beneath the private scratch root.
fn spooled_path(root: &Path, member: &str) -> io::Result<PathBuf> {
    let plaintext = member
        .strip_suffix(".gz")
        .ok_or_else(|| invalid("member path differs"))?;
    // reject every traversal component
    if plaintext
        .split('/')
        .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(invalid("member path differs"));
    }
    Ok(root.join(plaintext))
}

/// sort canonical JSONL rows by JavaScript's UTF-16 string ordering.
fn sort_member_lines(bytes: Vec<u8>) -> io::Result<Vec<u8>> {
    let text = String::from_utf8(bytes).map_err(io::Error::other)?;
    let mut lines = text
        .strip_suffix('\n')
        .ok_or_else(|| invalid("member plaintext framing differs"))?
        .split('\n')
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    // reject interior empty records
    if lines.iter().any(String::is_empty) {
        return Err(invalid("member plaintext row differs"));
    }
    lines.sort_by(|left, right| compare_utf16(left, right));
    let mut output = lines.join("\n").into_bytes();
    output.push(b'\n');
    Ok(output)
}

/// compare strings exactly as JavaScript's default array sort.
fn compare_utf16(left: &str, right: &str) -> Ordering {
    left.encode_utf16().cmp(right.encode_utf16())
}

/// Emit the exact predecessor outer gzip stream using the already installed host codec.
pub(crate) fn write_native_forecast_training_package_archive(
    files: BTreeMap<String, PathBuf>,
    mut output: impl Write,
) -> io::Result<()> {
    // GNU gzip's deflate output differs from Node's pinned zlib even for identical tar bytes
    let mut child = Command::new("gzip")
        .args(["--no-name", "--best"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    // reap the exact child even if a requested pipe cannot be retained
    let (Some(input), Some(mut archive)) = (child.stdin.take(), child.stdout.take()) else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(invalid("gzip pipes unavailable"));
    };
    // stream both sides with bounded pipes rather than retaining the complete outer archive
    std::thread::scope(|scope| {
        let producer =
            scope.spawn(move || write_native_forecast_training_package_tar(files, input));
        let streamed = io::copy(&mut archive, &mut output).and_then(|_| output.flush());
        // close only the process owned by this operation when the downstream consumer fails
        if streamed.is_err() {
            let _ = child.kill();
        }
        let emitted = producer.join();
        let status = child.wait()?;
        streamed?;
        emitted.map_err(|_| io::Error::other("tar producer stopped"))??;
        if !status.success() {
            return Err(io::Error::other("forecast training gzip failed"));
        }
        Ok(())
    })
}

/// Stream the package tree in the predecessor's deterministic sorted ustar shape.
fn write_native_forecast_training_package_tar(
    files: BTreeMap<String, PathBuf>,
    output: impl Write,
) -> io::Result<()> {
    let mut entries = BTreeMap::<String, Option<PathBuf>>::new();
    entries.insert("./".to_owned(), None);
    // derive every directory before inserting its file
    for (path, source) in files {
        let mut parent = String::new();
        let components = path.split('/').collect::<Vec<_>>();
        // retain each parent as a real tar entry
        for component in &components[..components.len().saturating_sub(1)] {
            parent.push_str(component);
            parent.push('/');
            entries.insert(format!("./{parent}"), None);
        }
        entries.insert(format!("./{path}"), Some(source));
    }
    let mut archive = NativeTrainingTar::new(output);
    // preserve GNU tar's bytewise sorted traversal
    for (path, source) in entries {
        archive.append(&path, source.as_deref())?;
    }
    archive.finish()
}

struct NativeTrainingTar<W: Write> {
    writer: W,
    tar_bytes: usize,
}

impl<W: Write> NativeTrainingTar<W> {
    /// Start the deterministic tar stream over the caller's bounded sink.
    fn new(output: W) -> Self {
        Self {
            writer: output,
            tar_bytes: 0,
        }
    }

    /// append one GNU-compatible ustar directory or regular file.
    fn append(&mut self, path: &str, source: Option<&Path>) -> io::Result<()> {
        let directory = source.is_none();
        let size = source
            .map(fs::metadata)
            .transpose()?
            .map_or(0, |metadata| metadata.len());
        let size = usize::try_from(size).map_err(io::Error::other)?;
        let (name, prefix) = split_tar_path(path)?;
        let mut header = [0_u8; TAR_BLOCK];
        header[..name.len()].copy_from_slice(name.as_bytes());
        tar_octal(&mut header[100..108], if directory { 0o700 } else { 0o600 })?;
        tar_octal(&mut header[108..116], 0)?;
        tar_octal(&mut header[116..124], 0)?;
        tar_octal(&mut header[124..136], size)?;
        tar_octal(&mut header[136..148], 0)?;
        header[148..156].fill(b' ');
        header[156] = if directory { b'5' } else { b'0' };
        header[257..262].copy_from_slice(b"ustar");
        header[263..265].copy_from_slice(b"00");
        header[345..345 + prefix.len()].copy_from_slice(prefix.as_bytes());
        let checksum: usize = header.iter().map(|byte| usize::from(*byte)).sum();
        header[148..156].copy_from_slice(format!("{checksum:06o}\0 ").as_bytes());
        self.writer.write_all(&header)?;
        // stream each regular member without retaining the complete archive body
        if let Some(source) = source {
            let copied = io::copy(&mut fs::File::open(source)?, &mut self.writer)?;
            // reject a concurrently changed source
            if copied != size as u64 {
                return Err(invalid("tar member size changed"));
            }
        }
        let padding = (TAR_BLOCK - size % TAR_BLOCK) % TAR_BLOCK;
        self.writer.write_all(&[0_u8; TAR_BLOCK][..padding])?;
        self.tar_bytes += TAR_BLOCK + size + padding;
        Ok(())
    }

    /// finish two empty blocks and GNU tar's fixed blocking record.
    fn finish(mut self) -> io::Result<()> {
        self.writer.write_all(&[0_u8; TAR_BLOCK * 2])?;
        self.tar_bytes += TAR_BLOCK * 2;
        let padding = (TAR_RECORD - self.tar_bytes % TAR_RECORD) % TAR_RECORD;
        self.writer.write_all(&vec![0_u8; padding])?;
        self.writer.flush()
    }
}

/// split one ustar path without using GNU long-name extensions.
fn split_tar_path(path: &str) -> io::Result<(&str, &str)> {
    // keep short paths wholly in the name field
    if path.len() <= 100 {
        return Ok((path, ""));
    }
    let (prefix, name) = path
        .rsplit_once('/')
        .ok_or_else(|| invalid("tar path differs"))?;
    // reject paths outside the ustar split fields
    if name.len() > 100 || prefix.len() > 155 {
        return Err(invalid("tar path limit"));
    }
    Ok((name, prefix))
}

/// encode one null-terminated ustar octal field.
fn tar_octal(field: &mut [u8], value: usize) -> io::Result<()> {
    let encoded = format!("{value:0width$o}", width = field.len() - 1);
    // reject values outside the fixed octal field
    if encoded.len() != field.len() - 1 {
        return Err(invalid("tar integer limit"));
    }
    field[..encoded.len()].copy_from_slice(encoded.as_bytes());
    Ok(())
}

struct ScratchDirectory(PathBuf);

impl ScratchDirectory {
    /// claim one fresh caller-selected private scratch directory.
    fn create(path: &Path) -> io::Result<Self> {
        fs::create_dir(path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
        Ok(Self(path.to_path_buf()))
    }

    /// borrow the process-owned scratch path.
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for ScratchDirectory {
    /// remove only the fresh process-owned scratch tree.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// project a canonical date through the same UTC-label ordinal as the predecessor.
fn date_ordinal(date: &str) -> Option<i64> {
    // retain only the canonical date width
    if date.len() != 10 {
        return None;
    }
    NativeWirePrimitives
        .canonical_utc_milliseconds(&format!("{date}T00:00:00.000Z"))
        .map(|milliseconds| milliseconds.div_euclid(86_400_000))
}

/// hash exact bytes into lowercase hexadecimal SHA-256.
fn sha256(bytes: &[u8]) -> String {
    RevisionArchiveCanonicalCodec::sha256(&NativeWirePrimitives, bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// build one generated JSON object from UTF-8 field names.
fn object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(name, value)| (name.encode_utf16().collect(), value))
            .collect(),
    )
}

/// build one generated JSON string.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// build one exact safe integer JSON number.
fn number(value: u64) -> JsonValue {
    JsonValue::Number(value as f64)
}

/// create one bounded native export error.
fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
