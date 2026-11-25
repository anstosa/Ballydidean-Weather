//! Admission of the retained legacy training-export manifest for wind fitting.

use weather_contracts::{ForecastObservationStationKey, JsonValue};
use weather_database::Sha256Digest;

use crate::maintenance_canonical_json::{exact_keys, field, text, valid_hash};
use crate::wind_maintenance_input::{
    ValidatedWindFitManifest, ValidatedWindFitMember, WindFitInputCalendar,
    manifest_training_provenance,
};

const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;
const MAXIMUM_ROWS: u64 = 4_000_000;
const QUERY_CONTRACT_SHA256: &str =
    "3b7926c47bbdb208ac2e305ee7798bfe4ea9590ce2863f556e752a71d1158e76";
const ROW_SCHEMA_SHA256: &str = "2717b6c3c704a1b52c7748b59c37d635efd92d92efb9dc97ea4ddef97cd504fc";
const AGGREGATION_CONTRACT_SHA256: &str =
    "9c309ef5a00780167570746ad6c31b9128c266db50954fe4645287e1f2b31e64";
const COORDINATE_MANIFEST_SHA256: &str =
    "04bfd93a03c393e977c8767a9aca6fe2a4cba9c263cb46e6987fa733b666ba58";
const METRIC_ELIGIBILITY_SHA256: &str =
    "53731954b347836a26500b05a195ca15cf26214c4d561fe482c5ff87ef56a82e";
const SOURCE_LINEAGE_SHA256: &str =
    "261a134589a12c1bbbd9a783343950317fd1fbc87e08383e60e805b7761566cc";
const STATION_MANIFEST_SHA256: &str =
    "a1f76440c056987bbb434d5315e4916f961deeb2951fe889d785943f559cdd49";
const SPATIAL_WEIGHTS_SHA256: &str =
    "8ed5ce70d33edd4a5166049d9938cbaaf800151b6a0b3345d3005419e9041c74";

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
const COVERAGE_KEYS: &[&str] = &["eligibleMetricNonNullLocalDates", "stationKey"];
const COVERAGE_METRIC_KEYS: &[&str] = &[
    "relative_humidity_percent",
    "temperature_c",
    "wind_direction_degrees",
    "wind_gust_mps",
    "wind_speed_mps",
];

/// Validate the closed retained snapshot boundary without opening any member file.
pub(crate) fn validate_snapshot_wind_fit_manifest(
    calendar: &impl WindFitInputCalendar,
    doc: &JsonValue,
) -> Option<ValidatedWindFitManifest> {
    exact_keys(doc, MANIFEST_KEYS).then_some(())?;
    validate_fixed_boundary(doc)?;
    let training_provenance = manifest_training_provenance(doc)?;
    // bind every legacy provenance identity to the frozen training manifest
    if training_provenance.aggregationContractSha256 != AGGREGATION_CONTRACT_SHA256
        || training_provenance.coordinateManifestSha256 != COORDINATE_MANIFEST_SHA256
        || training_provenance.metricEligibilitySha256 != METRIC_ELIGIBILITY_SHA256
        || training_provenance.observationSourceLineageSha256 != SOURCE_LINEAGE_SHA256
        || training_provenance.observationStationManifestSha256 != STATION_MANIFEST_SHA256
        || training_provenance.spatialWeightSha256 != SPATIAL_WEIGHTS_SHA256
    {
        return None;
    }

    let from_local_date = text(doc, "fromLocalDate")?;
    let to_local_date = text(doc, "toLocalDate")?;
    let from_ordinal = calendar.wind_fit_date_ordinal(&from_local_date)?;
    let to_ordinal = calendar.wind_fit_date_ordinal(&to_local_date)?;
    let maximum_dates = to_ordinal.checked_sub(from_ordinal)?.checked_add(1)?;
    // preserve the original inclusive nonempty calendar range
    if maximum_dates <= 0 {
        return None;
    }
    validate_station_coverage(doc, u64::try_from(maximum_dates).ok()?)?;
    let members = validate_members(calendar, doc)?;

    Some(ValidatedWindFitManifest {
        from_local_date,
        to_local_date,
        members,
        training_provenance,
    })
}

/// Validate the exact fixed identities and bounded nested manifest properties.
fn validate_fixed_boundary(doc: &JsonValue) -> Option<()> {
    let usage = object(doc, "usageBoundary")?;
    let limits = object(doc, "limits")?;
    let transaction = object(doc, "transaction")?;
    let database = object(doc, "databaseManifest")?;
    let query_contract_version = text(doc, "queryContractVersion")?;
    // preserve every literal legacy export and runtime identity
    if text(doc, "contractVersion")? != "forecast-training-export-package/v1"
        || text(doc, "siteKey")? != "ballydidean"
        || text(doc, "siteTimezone")? != "America/Los_Angeles"
        || boolean(usage, "databaseImportAllowed")?
        || !boolean(usage, "productionDerived")?
        || !boolean(usage, "snapshotOnly")?
        || query_contract_version != "forecast-training-export-query/v2"
        || text(doc, "queryContractSha256")? != QUERY_CONTRACT_SHA256
        || text(doc, "rowSchemaSha256")? != ROW_SCHEMA_SHA256
        || unsigned(limits, "maxDays")? != 450
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
        || text(database, "query_contract_version")? != query_contract_version
        || text(database, "schema_migration")? != "0010_forecast_training_export.sql"
        || !valid_hash(&text(doc, "migrationHistorySha256")?)
        || unsigned(doc, "totalRowCount")? > MAXIMUM_ROWS
    {
        return None;
    }
    let migration_names = array(database, "migration_names")?;
    let migration_checksums = array(database, "migration_checksums")?;
    // retain the original nonempty aligned migration ledger and exact digest spellings
    if migration_names.is_empty()
        || migration_names.len() != migration_checksums.len()
        || migration_checksums
            .iter()
            .any(|value| !string(value).is_some_and(|hash| valid_hash(&hash)))
        || !matches!(field(doc, "sourceIdentities")?, JsonValue::Array(_))
        || !matches!(field(doc, "observedSourceIdentities")?, JsonValue::Array(_))
    {
        return None;
    }
    Some(())
}

/// Validate exact station population, order, and every bounded metric-date count.
fn validate_station_coverage(doc: &JsonValue, maximum_dates: u64) -> Option<()> {
    let entries = array(doc, "stationMetricCoverage")?;
    let mut expected = ForecastObservationStationKey::WIRE_VALUES
        .iter()
        .map(|(wire, _)| *wire)
        .collect::<Vec<_>>();
    expected.sort_unstable();
    // require exactly one entry for every frozen station
    if entries.len() != expected.len() {
        return None;
    }
    let mut actual = Vec::with_capacity(entries.len());
    // retain source array order while checking each closed count object
    for entry in entries {
        exact_keys(entry, COVERAGE_KEYS).then_some(())?;
        let counts = field(entry, "eligibleMetricNonNullLocalDates")?;
        exact_keys(counts, COVERAGE_METRIC_KEYS).then_some(())?;
        // reject impossible, fractional, or over-window date counts
        for metric in COVERAGE_METRIC_KEYS {
            if unsigned(counts, metric)? > maximum_dates {
                return None;
            }
        }
        actual.push(text(entry, "stationKey")?);
    }
    (actual.iter().map(String::as_str).eq(expected)).then_some(())
}

/// Project exact member dates and digests while preserving canonical path order.
fn validate_members(
    calendar: &impl WindFitInputCalendar,
    doc: &JsonValue,
) -> Option<Vec<ValidatedWindFitMember>> {
    let entries = array(doc, "members")?;
    let expected_total = unsigned(doc, "totalRowCount")?;
    let mut row_total = 0_i128;
    let mut previous_path: Option<String> = None;
    let mut members = Vec::with_capacity(entries.len());
    // retain the manifest's declared order for later member-to-receipt joins
    for entry in entries {
        let path = text(entry, "path")?;
        // preserve the source's strict canonical member ordering
        if previous_path
            .as_deref()
            .is_some_and(|previous| previous >= path.as_str())
        {
            return None;
        }
        previous_path = Some(path);
        row_total = row_total.checked_add(i128::from(safe_integer(entry, "rowCount")?))?;
        let local_date = text(entry, "localDate")?;
        calendar.wind_fit_date_ordinal(&local_date)?;
        let sha256 = text(entry, "sha256")?;
        // project only identities admitted by the original lowercase digest contract
        if !valid_hash(&sha256) {
            return None;
        }
        members.push(ValidatedWindFitMember {
            local_date,
            sha256: Sha256Digest::admitted(sha256),
        });
    }
    (row_total == i128::from(expected_total)).then_some(members)
}

/// Borrow one required JSON object.
fn object<'a>(doc: &'a JsonValue, key: &str) -> Option<&'a JsonValue> {
    let value = field(doc, key)?;
    matches!(value, JsonValue::Object(_)).then_some(value)
}

/// Borrow one required JSON array.
fn array<'a>(doc: &'a JsonValue, key: &str) -> Option<&'a [JsonValue]> {
    let JsonValue::Array(values) = field(doc, key)? else {
        return None;
    };
    Some(values)
}

/// Read one required JSON boolean without coercion.
fn boolean(doc: &JsonValue, key: &str) -> Option<bool> {
    let JsonValue::Boolean(value) = field(doc, key)? else {
        return None;
    };
    Some(*value)
}

/// Read one UTF-16 JSON string as an exact Rust string.
fn string(value: &JsonValue) -> Option<String> {
    let JsonValue::String(units) = value else {
        return None;
    };
    String::from_utf16(units).ok()
}

/// Read one nonnegative JavaScript-safe integer.
fn unsigned(doc: &JsonValue, key: &str) -> Option<u64> {
    let value = safe_integer(doc, key)?;
    u64::try_from(value).ok()
}

/// Read one signed JavaScript-safe integer for exact member-row summation.
fn safe_integer(doc: &JsonValue, key: &str) -> Option<i64> {
    let JsonValue::Number(value) = field(doc, key)? else {
        return None;
    };
    (value.is_finite()
        && value.fract() == 0.0
        && (-MAX_SAFE_INTEGER..=MAX_SAFE_INTEGER).contains(value))
    .then_some(*value as i64)
}
