//! Admission of the original forecast-training package manifest contract.

use weather_contracts::{ForecastObservationStationKey, JsonValue};
use weather_database::Sha256Digest;

use crate::MaintenanceCanonicalJson;
use crate::forecast_training_package_row::{
    FORECAST_TRAINING_SOURCE_IDENTITIES, ForecastTrainingRecordKind,
};
use crate::maintenance_canonical_json::{exact_keys, field, text, valid_hash};

const PACKAGE_VERSION: &str = "forecast-training-export-package/v1";
const MANIFEST_VERSION: &str = "forecast-training-export-manifest/v1";
const QUERY_CONTRACT_VERSION: &str = "forecast-training-export-query/v2";
const SCHEMA_MIGRATION: &str = "0010_forecast_training_export.sql";
const MAXIMUM_DAYS: i64 = 450;
const MAXIMUM_ROWS: u64 = 4_000_000;
const MAXIMUM_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

const MANIFEST_KEYS: &[&str] = &[
    "aggregationContractSha256",
    "contractVersion",
    "coordinateManifestSha256",
    "createdAtUtc",
    "databaseManifest",
    "fromLocalDate",
    "limits",
    "members",
    "metricEligibilitySha256",
    "migrationHistorySha256",
    "observedSourceIdentities",
    "queryContractSha256",
    "queryContractVersion",
    "rowSchemaSha256",
    "siteKey",
    "siteTimezone",
    "sourceIdentities",
    "sourceLineageSha256",
    "spatialWeightsSha256",
    "stationMetricCoverage",
    "stationManifestSha256",
    "toLocalDate",
    "totalRowCount",
    "transaction",
    "usageBoundary",
];
const DATABASE_MANIFEST_KEYS: &[&str] = &[
    "aggregation_contract_sha256",
    "contract_version",
    "coordinate_manifest_sha256",
    "metric_eligibility_sha256",
    "migration_checksums",
    "migration_history_sha256",
    "migration_names",
    "query_contract_sha256",
    "query_contract_version",
    "row_schema_sha256",
    "schema_migration",
    "site_key",
    "site_timezone",
    "source_lineage_sha256",
    "spatial_weights_sha256",
    "station_manifest_sha256",
];
const LIMIT_KEYS: &[&str] = &[
    "conservativeExportRowFormula",
    "conservativeExportRows",
    "exportRowHeadroom",
    "maxDays",
    "maxRows",
    "rowCountMeaning",
];
const TRANSACTION_KEYS: &[&str] = &[
    "idleInTransactionSessionTimeout",
    "isolationLevel",
    "lockTimeout",
    "readOnly",
    "statementTimeout",
];
const USAGE_KEYS: &[&str] = &["databaseImportAllowed", "productionDerived", "snapshotOnly"];
const MEMBER_KEYS: &[&str] = &[
    "localDate",
    "maxValidAt",
    "minValidAt",
    "path",
    "plaintextBytes",
    "recordKind",
    "rowCount",
    "sha256",
    "sizeBytes",
    "stationKey",
];
const SOURCE_IDENTITY_KEYS: &[&str] = &["adapterContract", "sourceConfigFingerprint", "sourceKey"];
const COVERAGE_KEYS: &[&str] = &["eligibleMetricNonNullLocalDates", "stationKey"];

const EXPECTED_HASHES: &[(&str, &str, &str)] = &[
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

const MIGRATIONS: &[(&str, &str)] = &[
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
    (
        "0014_rain_collection.sql",
        "2a311e2effcc975442c1011c627c125521cc4396323b32f0d0c4f92ea1ccba04",
    ),
    (
        "0015_rain_station_access.sql",
        "2f560ccec001246a3caa5d27476900b2694dfb7cc0701b35181265ef1fb78822",
    ),
    (
        "0016_rain_adjustment.sql",
        "9fa5659c032dc21fdf82dca693fa962d8b5212d1192ae35c4cfd04bef94be5f9",
    ),
    (
        "0017_adjustment_evaluation_export.sql",
        "6f18210453a18f0deefe95a0b70657cc4c4dd3fc20d0bab72f847ee04958ab90",
    ),
];

/// Frozen station order retained by the TypeScript manifest schema.
pub const FORECAST_TRAINING_STATION_KEYS: &[(&str, ForecastObservationStationKey)] =
    ForecastObservationStationKey::WIRE_VALUES;

/// Frozen metric order retained by each station-coverage object.
pub const FORECAST_TRAINING_METRIC_NAMES: [&str; 5] = [
    "relative_humidity_percent",
    "temperature_c",
    "wind_direction_degrees",
    "wind_gust_mps",
    "wind_speed_mps",
];

/// Approved date and UTC parsing owner used by manifest admission.
pub trait ForecastTrainingPackageManifestValues: MaintenanceCanonicalJson {
    /// Return a monotonic ordinal for one real canonical local date.
    fn local_date_ordinal(&mut self, date: &str) -> Option<i64>;

    /// Return the epoch milliseconds for one real UTC instant.
    fn utc_epoch_milliseconds(&mut self, instant: &str) -> Option<i64>;
}

/// One source identity shared with row admission after frozen-roster matching.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForecastTrainingPackageSourceIdentity {
    pub adapter_contract: &'static str,
    pub source_config_fingerprint: &'static str,
    pub source_key: &'static str,
}

/// One declared package member admitted before its payload is opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForecastTrainingPackageMember {
    pub local_date: String,
    pub maximum_valid_at: String,
    pub minimum_valid_at: String,
    pub path: String,
    pub plaintext_bytes: u64,
    pub record_kind: ForecastTrainingRecordKind,
    pub row_count: u64,
    pub sha256: Sha256Digest,
    pub size_bytes: u64,
    pub station_key: Option<String>,
}

/// The five sanitized non-null local-date upper bounds for one station.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForecastTrainingPackageMetricCounts {
    pub relative_humidity_percent: u64,
    pub temperature_c: u64,
    pub wind_direction_degrees: u64,
    pub wind_gust_mps: u64,
    pub wind_speed_mps: u64,
}

/// One station's sanitized metric-date coverage evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForecastTrainingPackageStationMetricCoverage {
    pub station_key: ForecastObservationStationKey,
    pub eligible_metric_non_null_local_dates: ForecastTrainingPackageMetricCounts,
}

impl ForecastTrainingPackageStationMetricCoverage {
    /// Return the generated wire spelling retained by this entry.
    pub fn station_key_wire(&self) -> &'static str {
        FORECAST_TRAINING_STATION_KEYS
            .iter()
            .find_map(|(wire, station)| (*station == self.station_key).then_some(*wire))
            .expect("admitted station key has a generated wire value")
    }
}

/// Complete typed evidence retained from an admitted package manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedForecastTrainingPackageManifest {
    pub created_at_utc: String,
    pub from_local_date: String,
    pub inclusive_date_count: u64,
    pub members: Vec<ForecastTrainingPackageMember>,
    pub observed_source_identities: Vec<ForecastTrainingPackageSourceIdentity>,
    pub source_identities: Vec<ForecastTrainingPackageSourceIdentity>,
    pub station_metric_coverage: Vec<ForecastTrainingPackageStationMetricCoverage>,
    pub to_local_date: String,
    pub total_row_count: u64,
}

/// Admit the original package manifest without opening any declared member.
pub fn admit_forecast_training_package_manifest(
    values: &mut impl ForecastTrainingPackageManifestValues,
    manifest: &JsonValue,
) -> Option<AdmittedForecastTrainingPackageManifest> {
    exact_keys(manifest, MANIFEST_KEYS).then_some(())?;
    validate_fixed_boundary(manifest)?;

    let database = object(manifest, "databaseManifest")?;
    validate_database_manifest(values, database)?;
    validate_duplicate_hashes(manifest, database)?;

    let created_at_utc = text(manifest, "createdAtUtc")?;
    valid_utc_instant(values, &created_at_utc, false).then_some(())?;
    let from_local_date = text(manifest, "fromLocalDate")?;
    let to_local_date = text(manifest, "toLocalDate")?;
    let from_ordinal = values.local_date_ordinal(&from_local_date)?;
    let to_ordinal = values.local_date_ordinal(&to_local_date)?;
    let inclusive_date_count = to_ordinal.checked_sub(from_ordinal)?.checked_add(1)?;

    // retain the original inclusive 1..=450 date boundary
    if !(1..=MAXIMUM_DAYS).contains(&inclusive_date_count) {
        return None;
    }

    let total_row_count = unsigned(manifest, "totalRowCount")?;

    // retain the original hard export-row ceiling
    if total_row_count > MAXIMUM_ROWS {
        return None;
    }

    let source_identities = validate_source_identities(manifest, "sourceIdentities")?;
    validate_full_source_identity_roster(&source_identities)?;
    let observed_source_identities =
        validate_source_identities(manifest, "observedSourceIdentities")?;
    validate_observed_source_identities(&observed_source_identities)?;
    let station_metric_coverage =
        validate_station_metric_coverage(manifest, inclusive_date_count as u64)?;
    let members = validate_members(values, manifest, from_ordinal, to_ordinal, total_row_count)?;

    Some(AdmittedForecastTrainingPackageManifest {
        created_at_utc,
        from_local_date,
        inclusive_date_count: inclusive_date_count as u64,
        members,
        observed_source_identities,
        source_identities,
        station_metric_coverage,
        to_local_date,
        total_row_count,
    })
}

/// Validate fixed package literals and exact closed nested shapes.
fn validate_fixed_boundary(manifest: &JsonValue) -> Option<()> {
    let limits = object(manifest, "limits")?;
    let transaction = object(manifest, "transaction")?;
    let usage = object(manifest, "usageBoundary")?;

    // bind the complete immutable package boundary
    if !exact_keys(limits, LIMIT_KEYS)
        || !exact_keys(transaction, TRANSACTION_KEYS)
        || !exact_keys(usage, USAGE_KEYS)
        || text(manifest, "contractVersion")? != PACKAGE_VERSION
        || text(manifest, "siteKey")? != "ballydidean"
        || text(manifest, "siteTimezone")? != "America/Los_Angeles"
        || text(manifest, "queryContractVersion")? != QUERY_CONTRACT_VERSION
        || boolean(usage, "databaseImportAllowed")?
        || !boolean(usage, "productionDerived")?
        || !boolean(usage, "snapshotOnly")?
        || unsigned(limits, "maxDays")? != MAXIMUM_DAYS as u64
        || unsigned(limits, "maxRows")? != MAXIMUM_ROWS
        || unsigned(limits, "conservativeExportRows")? != 3_045_600
        || unsigned(limits, "exportRowHeadroom")? != 954_400
        || text(limits, "rowCountMeaning")? != "export_rows_not_training_events"
        || text(limits, "conservativeExportRowFormula")? != "450 * ((24 * 264) + (11 * 24) + 168)"
        || text(transaction, "readOnly")? != "on"
        || text(transaction, "isolationLevel")? != "repeatable read"
        || text(transaction, "statementTimeout")? != "15min"
        || text(transaction, "lockTimeout")? != "5s"
        || text(transaction, "idleInTransactionSessionTimeout")? != "30s"
    {
        return None;
    }

    Some(())
}

/// Validate the frozen upstream manifest and its approved migration ledger.
fn validate_database_manifest(
    values: &mut impl ForecastTrainingPackageManifestValues,
    database: &JsonValue,
) -> Option<()> {
    // bind the original upstream identity and exact shape
    if !exact_keys(database, DATABASE_MANIFEST_KEYS)
        || text(database, "contract_version")? != MANIFEST_VERSION
        || text(database, "site_key")? != "ballydidean"
        || text(database, "site_timezone")? != "America/Los_Angeles"
        || text(database, "query_contract_version")? != QUERY_CONTRACT_VERSION
        || text(database, "schema_migration")? != SCHEMA_MIGRATION
    {
        return None;
    }

    // bind every immutable upstream contract digest
    for (_, database_key, expected) in EXPECTED_HASHES {
        let actual = text(database, database_key)?;

        // reject malformed or self-consistently substituted hashes
        if !valid_hash(&actual) || actual != *expected {
            return None;
        }
    }

    let migration_history = text(database, "migration_history_sha256")?;

    // retain the original digest spelling before computing its preimage
    if !valid_hash(&migration_history) {
        return None;
    }

    let names = string_array(database, "migration_names")?;
    let checksums = string_array(database, "migration_checksums")?;

    // accept only one exact reviewed 13-through-17 migration prefix
    if names.len() != checksums.len()
        || !(13..=MIGRATIONS.len()).contains(&names.len())
        || !names
            .iter()
            .zip(&checksums)
            .zip(MIGRATIONS)
            .all(|((name, checksum), expected)| {
                name == expected.0 && checksum == expected.1 && valid_hash(checksum)
            })
    {
        return None;
    }

    let mut preimage = String::new();

    // reproduce the original newline-separated ledger with no final LF
    for (index, (name, checksum)) in names.iter().zip(&checksums).enumerate() {
        // separate entries without terminating the complete preimage
        if index != 0 {
            preimage.push('\n');
        }
        preimage.push_str(name);
        preimage.push(':');
        preimage.push_str(checksum);
    }

    let computed = values.sha256(preimage.as_bytes())?;
    (computed.as_str() == migration_history).then_some(())
}

/// Bind all package hash duplicates to the validated upstream manifest.
fn validate_duplicate_hashes(manifest: &JsonValue, database: &JsonValue) -> Option<()> {
    // require every frozen package hash to equal its upstream duplicate
    for (package_key, database_key, expected) in EXPECTED_HASHES {
        let package_value = text(manifest, package_key)?;
        let database_value = text(database, database_key)?;

        // reject malformed, substituted, or cross-mismatched package hashes
        if !valid_hash(&package_value)
            || package_value != database_value
            || package_value != *expected
        {
            return None;
        }
    }

    let package_history = text(manifest, "migrationHistorySha256")?;
    let database_history = text(database, "migration_history_sha256")?;

    // bind the remaining duplicate values exactly
    if !valid_hash(&package_history)
        || package_history != database_history
        || text(manifest, "queryContractVersion")? != text(database, "query_contract_version")?
    {
        return None;
    }

    Some(())
}

/// Validate and retain every declared package member in source order.
fn validate_members(
    values: &mut impl ForecastTrainingPackageManifestValues,
    manifest: &JsonValue,
    from_ordinal: i64,
    to_ordinal: i64,
    expected_total: u64,
) -> Option<Vec<ForecastTrainingPackageMember>> {
    let entries = array(manifest, "members")?;
    let mut members = Vec::with_capacity(entries.len());
    let mut previous_path: Option<String> = None;
    let mut declared_total = 0_u64;

    // retain every member in the original nondecreasing path order
    for entry in entries {
        exact_keys(entry, MEMBER_KEYS).then_some(())?;
        let path = text(entry, "path")?;

        // preserve JavaScript sort semantics, including equal adjacent paths
        if previous_path
            .as_deref()
            .is_some_and(|previous| previous > path.as_str())
        {
            return None;
        }

        let local_date = text(entry, "localDate")?;
        let ordinal = values.local_date_ordinal(&local_date)?;

        // bind every member date to the declared package interval
        if !(from_ordinal..=to_ordinal).contains(&ordinal) {
            return None;
        }

        let record_kind_text = text(entry, "recordKind")?;
        let record_kind = record_kind(&record_kind_text)?;
        let station_key = nullable_text(entry, "stationKey")?;
        validate_member_path(&path, &local_date, record_kind, station_key.as_deref())?;

        let minimum_valid_at = text(entry, "minValidAt")?;
        let maximum_valid_at = text(entry, "maxValidAt")?;

        // member bounds originate from normalized validated row instants
        if !valid_utc_instant(values, &minimum_valid_at, true)
            || !valid_utc_instant(values, &maximum_valid_at, true)
            || minimum_valid_at > maximum_valid_at
        {
            return None;
        }

        let plaintext_bytes = unsigned(entry, "plaintextBytes")?;
        let row_count = unsigned(entry, "rowCount")?;
        let size_bytes = unsigned(entry, "sizeBytes")?;
        let sha256 = text(entry, "sha256")?;

        // each original member is a nonempty compressed row population
        if plaintext_bytes == 0 || row_count == 0 || size_bytes == 0 || !valid_hash(&sha256) {
            return None;
        }

        declared_total = declared_total.checked_add(row_count)?;
        previous_path = Some(path.clone());
        members.push(ForecastTrainingPackageMember {
            local_date,
            maximum_valid_at,
            minimum_valid_at,
            path,
            plaintext_bytes,
            record_kind,
            row_count,
            sha256: Sha256Digest::admitted(sha256),
            size_bytes,
            station_key,
        });
    }

    // preserve the original eventual member-to-package row-count binding
    (declared_total == expected_total).then_some(members)
}

/// Validate one original member path and its partition identity.
fn validate_member_path(
    path: &str,
    local_date: &str,
    record_kind: ForecastTrainingRecordKind,
    station_key: Option<&str>,
) -> Option<()> {
    let suffix = path.strip_prefix("members/")?;
    let mut parts = suffix.split('/');
    let path_date = parts.next()?;
    let path_kind = parts.next()?;
    let filename = parts.next()?;

    // require exactly three original partition components without traversal
    if parts.next().is_some()
        || path.contains("..")
        || path_date != local_date
        || path_kind != record_kind.member_component()
    {
        return None;
    }

    let leaf = filename.strip_suffix(".jsonl.gz")?;

    // retain the original safe leaf-component grammar
    if !safe_component(leaf) {
        return None;
    }

    // select the original station or forecast partition identity
    match record_kind {
        ForecastTrainingRecordKind::StationHour => {
            let station_key = station_key?;

            // bind station partitions to one generated station identity
            if station_key != leaf || station(station_key).is_none() {
                return None;
            }
        }
        ForecastTrainingRecordKind::FixedLeadAnchor
        | ForecastTrainingRecordKind::LegacyV4RetrievalSnapshot => {
            // forecast partitions retain the fixed null station identity
            if station_key.is_some() || leaf != "forecast" {
                return None;
            }
        }
    }

    Some(())
}

/// Validate and retain one complete source-identity list.
fn validate_source_identities(
    manifest: &JsonValue,
    key: &str,
) -> Option<Vec<ForecastTrainingPackageSourceIdentity>> {
    let entries = array(manifest, key)?;
    let mut identities = Vec::with_capacity(entries.len());

    // bind each decoded declaration to one frozen source triple
    for entry in entries {
        exact_keys(entry, SOURCE_IDENTITY_KEYS).then_some(())?;
        let adapter_contract = text(entry, "adapterContract")?;
        let source_config_fingerprint = text(entry, "sourceConfigFingerprint")?;
        let source_key = text(entry, "sourceKey")?;
        let identity = FORECAST_TRAINING_SOURCE_IDENTITIES
            .iter()
            .find(|expected| {
                adapter_contract == expected.adapter_contract
                    && source_config_fingerprint == expected.source_config_fingerprint
                    && source_key == expected.source_key
            })?;
        identities.push(*identity);
    }

    Some(identities)
}

/// Bind the complete source list to the row validator's frozen authority.
fn validate_full_source_identity_roster(
    identities: &[ForecastTrainingPackageSourceIdentity],
) -> Option<()> {
    // require exact source population, order, and immutable triple values
    if identities != FORECAST_TRAINING_SOURCE_IDENTITIES {
        return None;
    }

    Some(())
}

/// Validate the sorted observed subset before payload verification.
fn validate_observed_source_identities(
    identities: &[ForecastTrainingPackageSourceIdentity],
) -> Option<()> {
    let mut previous_source_key: Option<&str> = None;

    // preserve original subset membership and nondecreasing source-key order
    for identity in identities {
        // reject reversed observed identities while retaining original duplicate semantics
        if previous_source_key.is_some_and(|previous| previous > identity.source_key) {
            return None;
        }
        previous_source_key = Some(identity.source_key);
    }

    Some(())
}

/// Validate the complete ordered station metric-coverage evidence.
fn validate_station_metric_coverage(
    manifest: &JsonValue,
    maximum_dates: u64,
) -> Option<Vec<ForecastTrainingPackageStationMetricCoverage>> {
    let entries = array(manifest, "stationMetricCoverage")?;

    // require exactly one entry for every generated station identity
    if entries.len() != FORECAST_TRAINING_STATION_KEYS.len() {
        return None;
    }

    let mut coverage = Vec::with_capacity(entries.len());

    // bind station order and every bounded metric count
    for (entry, (expected_wire, expected_station)) in
        entries.iter().zip(FORECAST_TRAINING_STATION_KEYS)
    {
        exact_keys(entry, COVERAGE_KEYS).then_some(())?;
        let station_key = text(entry, "stationKey")?;

        // retain the original canonical station order
        if station_key != *expected_wire {
            return None;
        }

        let counts = object(entry, "eligibleMetricNonNullLocalDates")?;
        exact_keys(counts, &FORECAST_TRAINING_METRIC_NAMES).then_some(())?;
        let counts = ForecastTrainingPackageMetricCounts {
            relative_humidity_percent: unsigned(counts, "relative_humidity_percent")?,
            temperature_c: unsigned(counts, "temperature_c")?,
            wind_direction_degrees: unsigned(counts, "wind_direction_degrees")?,
            wind_gust_mps: unsigned(counts, "wind_gust_mps")?,
            wind_speed_mps: unsigned(counts, "wind_speed_mps")?,
        };

        // reject impossible metric-date upper bounds
        if metric_counts(&counts)
            .into_iter()
            .any(|count| count > maximum_dates)
        {
            return None;
        }

        coverage.push(ForecastTrainingPackageStationMetricCoverage {
            station_key: *expected_station,
            eligible_metric_non_null_local_dates: counts,
        });
    }

    Some(coverage)
}

/// Return the fixed count fields for bounded validation.
fn metric_counts(counts: &ForecastTrainingPackageMetricCounts) -> [u64; 5] {
    [
        counts.relative_humidity_percent,
        counts.temperature_c,
        counts.wind_direction_degrees,
        counts.wind_gust_mps,
        counts.wind_speed_mps,
    ]
}

/// Recognize one generated station wire identity.
fn station(value: &str) -> Option<ForecastObservationStationKey> {
    FORECAST_TRAINING_STATION_KEYS
        .iter()
        .find_map(|(wire, station)| (*wire == value).then_some(*station))
}

/// Recognize one original manifest record-kind spelling.
fn record_kind(value: &str) -> Option<ForecastTrainingRecordKind> {
    [
        ForecastTrainingRecordKind::StationHour,
        ForecastTrainingRecordKind::FixedLeadAnchor,
        ForecastTrainingRecordKind::LegacyV4RetrievalSnapshot,
    ]
    .into_iter()
    .find(|kind| kind.member_component() == value)
}

/// Recognize the original ASCII safe-component grammar.
fn safe_component(value: &str) -> bool {
    let bytes = value.as_bytes();
    let Some(first) = bytes.first() else {
        return false;
    };

    // require one lowercase letter or digit followed by at most 127 allowed bytes
    bytes.len() <= 128
        && (first.is_ascii_lowercase() || first.is_ascii_digit())
        && bytes.iter().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        })
}

/// Recognize one real UTC instant with the original textual grammar.
fn valid_utc_instant(
    values: &mut impl ForecastTrainingPackageManifestValues,
    instant: &str,
    canonical_milliseconds: bool,
) -> bool {
    let bytes = instant.as_bytes();
    // distinguish normalized member bounds from the broader created-at grammar
    let length_ok = if canonical_milliseconds {
        bytes.len() == 24 && bytes.get(19) == Some(&b'.')
    } else {
        matches!(bytes.len(), 20 | 22..=27) && (bytes.len() == 20 || bytes.get(19) == Some(&b'.'))
    };

    // preserve the exact UTC punctuation and fractional-second width
    if !length_ok
        || bytes.get(4) != Some(&b'-')
        || bytes.get(7) != Some(&b'-')
        || bytes.get(10) != Some(&b'T')
        || bytes.get(13) != Some(&b':')
        || bytes.get(16) != Some(&b':')
        || bytes.last() != Some(&b'Z')
        || bytes.iter().enumerate().any(|(index, byte)| {
            !matches!(index, 4 | 7 | 10 | 13 | 16 | 19)
                && index + 1 != bytes.len()
                && !byte.is_ascii_digit()
        })
    {
        return false;
    }

    let date = instant.get(..10);
    let hour = instant
        .get(11..13)
        .and_then(|value| value.parse::<u8>().ok());
    let minute = instant
        .get(14..16)
        .and_then(|value| value.parse::<u8>().ok());
    let second = instant
        .get(17..19)
        .and_then(|value| value.parse::<u8>().ok());

    // reject calendar and clock rollover before accepting the parsed epoch
    date.is_some_and(|value| values.local_date_ordinal(value).is_some())
        && hour.is_some_and(|value| value < 24)
        && minute.is_some_and(|value| value < 60)
        && second.is_some_and(|value| value < 60)
        && values.utc_epoch_milliseconds(instant).is_some()
}

/// Borrow one required nested object.
fn object<'a>(document: &'a JsonValue, key: &str) -> Option<&'a JsonValue> {
    let value = field(document, key)?;
    matches!(value, JsonValue::Object(_)).then_some(value)
}

/// Borrow one required nested array.
fn array<'a>(document: &'a JsonValue, key: &str) -> Option<&'a [JsonValue]> {
    let JsonValue::Array(values) = field(document, key)? else {
        return None;
    };
    Some(values)
}

/// Decode one required array of exact UTF-16 strings.
fn string_array(document: &JsonValue, key: &str) -> Option<Vec<String>> {
    let values = array(document, key)?;
    let mut output = Vec::with_capacity(values.len());

    // decode every string without replacement
    for value in values {
        let JsonValue::String(units) = value else {
            return None;
        };
        output.push(String::from_utf16(units).ok()?);
    }

    Some(output)
}

/// Read one required exact boolean.
fn boolean(document: &JsonValue, key: &str) -> Option<bool> {
    let JsonValue::Boolean(value) = field(document, key)? else {
        return None;
    };
    Some(*value)
}

/// Read one nullable exact UTF-16 string.
fn nullable_text(document: &JsonValue, key: &str) -> Option<Option<String>> {
    // preserve the original explicit null-or-string schema
    match field(document, key)? {
        JsonValue::Null => Some(None),
        JsonValue::String(units) => Some(Some(String::from_utf16(units).ok()?)),
        _ => None,
    }
}

/// Read one nonnegative JavaScript-safe integer.
fn unsigned(document: &JsonValue, key: &str) -> Option<u64> {
    let JsonValue::Number(value) = field(document, key)? else {
        return None;
    };

    // preserve Number.isSafeInteger and nonnegative count semantics
    if !value.is_finite() || value.fract() != 0.0 || *value < 0.0 || *value > MAXIMUM_SAFE_INTEGER {
        return None;
    }

    Some(*value as u64)
}

#[cfg(test)]
#[path = "forecast_training_package_manifest_fixture_test.rs"]
mod fixture_tests;
