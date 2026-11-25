//! Verify one complete extracted forecast-training package before any downstream use.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Display};

use weather_contracts::{ForecastObservationStationKey, JsonValue};

use crate::forecast_training_package_manifest::{
    AdmittedForecastTrainingPackageManifest, ForecastTrainingPackageManifestValues,
    ForecastTrainingPackageSourceIdentity, admit_forecast_training_package_manifest,
};
use crate::forecast_training_package_row::{
    ForecastTrainingPackageRowError, ForecastTrainingPackageRowTime, ForecastTrainingRecordKind,
    ForecastTrainingRowMetrics, validate_forecast_training_package_row,
};

const MANIFEST_PATH: &str = "manifest.json";
const MANIFEST_CHECKSUM_PATH: &str = "manifest.sha256";

/// Approved path, file, JSON, compression, digest, and calendar primitives.
pub trait ForecastTrainingPackageVerificationValues:
    ForecastTrainingPackageManifestValues + ForecastTrainingPackageRowTime
{
    type Error: Display;

    /// Resolve one caller path without admitting its filesystem identity.
    fn absolute_path(&mut self, raw: &str) -> Result<String, Self::Error>;

    /// return the real path for an existing nonsymbolic package path
    fn canonical_package_path(&mut self, absolute: &str) -> Result<Option<String>, Self::Error>;

    /// Read one existing nonsymbolic regular file beneath the admitted root.
    fn read_regular_file(
        &mut self,
        root: &str,
        relative_path: &str,
    ) -> Result<Option<Vec<u8>>, Self::Error>;

    /// Enumerate every nonsymbolic regular file and real directory relative to the root.
    fn collect_regular_tree_entries(
        &mut self,
        root: &str,
    ) -> Result<Option<Vec<String>>, Self::Error>;

    /// Decode one ordinary JSON value without owning its validation rules.
    fn decode_json(&mut self, bytes: &[u8]) -> Option<JsonValue>;

    /// Inflate one complete gzip member with the approved compression owner.
    fn gunzip(&mut self, compressed: &[u8]) -> Result<Vec<u8>, Self::Error>;

    /// Enumerate exact inclusive local-date labels with the approved calendar owner.
    fn inclusive_local_dates(&mut self, from: &str, to: &str) -> Option<Vec<String>>;

    /// Return the exact UTC hourly instants belonging to one Los Angeles local date.
    fn local_date_utc_hours(&mut self, local_date: &str) -> Option<Vec<i64>>;
}

/// Closed ordinary failures from forecast-training package verification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForecastTrainingPackageVerificationError {
    Effect(String),
    RootInvalid,
    RootNoncanonical,
    ControlFiles,
    ManifestChecksum,
    Manifest,
    ManifestCanonical,
    ManifestBoundary,
    MemberLeaf,
    MemberPath,
    MemberMissing,
    MemberChecksum,
    MemberPlaintext,
    MemberOrder,
    RowJson,
    Row(ForecastTrainingPackageRowError),
    RowPartition,
    MemberBounds,
    TotalRows,
    StationGrid,
    SourceIdentities,
    StationCoverage,
    Inventory,
}

impl Display for ForecastTrainingPackageVerificationError {
    /// Preserve the predecessor verifier diagnostics at the command boundary.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // retain one exact message for every closed verification phase
        match self {
            Self::Effect(message) => formatter.write_str(message),
            Self::RootInvalid => formatter.write_str("package root is missing or symbolic"),
            Self::RootNoncanonical => formatter.write_str("package root must be canonical"),
            Self::ControlFiles => formatter.write_str("package control files are invalid"),
            Self::ManifestChecksum => formatter.write_str("manifest checksum mismatch"),
            Self::Manifest => formatter.write_str("package manifest must be an object"),
            Self::ManifestCanonical => {
                formatter.write_str("package manifest is not canonical JSON")
            }
            Self::ManifestBoundary => formatter.write_str("package manifest boundary is invalid"),
            Self::MemberLeaf => formatter.write_str("member leaf is invalid"),
            Self::MemberPath => formatter.write_str("member path is invalid"),
            Self::MemberMissing => formatter.write_str("member is missing or symbolic"),
            Self::MemberChecksum => formatter.write_str("member checksum or size mismatch"),
            Self::MemberPlaintext => formatter.write_str("member count or plaintext size mismatch"),
            Self::MemberOrder => formatter.write_str("member rows are not canonically ordered"),
            Self::RowJson => formatter.write_str("export row is invalid JSON"),
            Self::Row(error) => Display::fmt(error, formatter),
            Self::RowPartition => {
                formatter.write_str("row is stored in the wrong member partition")
            }
            Self::MemberBounds => formatter.write_str("member date bounds mismatch"),
            Self::TotalRows => formatter.write_str("package total row count mismatch"),
            Self::StationGrid => formatter.write_str("package station-hour grid is incomplete"),
            Self::SourceIdentities => formatter.write_str("package source identities mismatch"),
            Self::StationCoverage => {
                formatter.write_str("package station metric coverage mismatch")
            }
            Self::Inventory => formatter.write_str("package file inventory mismatch"),
        }
    }
}

impl std::error::Error for ForecastTrainingPackageVerificationError {}

/// Verify one extracted package and return the SHA-256 of its exact canonical manifest bytes.
pub fn verify_forecast_training_package(
    root: &str,
    values: &mut impl ForecastTrainingPackageVerificationValues,
) -> Result<String, ForecastTrainingPackageVerificationError> {
    let absolute_root = values.absolute_path(root).map_err(effect)?;
    let canonical_root = values
        .canonical_package_path(&absolute_root)
        .map_err(effect)?
        .ok_or(ForecastTrainingPackageVerificationError::RootInvalid)?;

    // require the resolved caller path to be the directory's real identity
    if canonical_root != absolute_root {
        return Err(ForecastTrainingPackageVerificationError::RootNoncanonical);
    }

    let manifest_bytes = read_control_file(values, &absolute_root, MANIFEST_PATH)?;
    let checksum_bytes = read_control_file(values, &absolute_root, MANIFEST_CHECKSUM_PATH)?;
    let manifest_digest = values
        .sha256(&manifest_bytes)
        .ok_or(ForecastTrainingPackageVerificationError::ManifestChecksum)?;
    let expected_checksum = format!("{}  {MANIFEST_PATH}\n", manifest_digest.as_str());

    // bind the checksum control file to the exact manifest bytes
    if checksum_bytes != expected_checksum.as_bytes() {
        return Err(ForecastTrainingPackageVerificationError::ManifestChecksum);
    }

    let manifest = values
        .parse_canonical_object(&manifest_bytes)
        .ok_or(ForecastTrainingPackageVerificationError::Manifest)?;
    let canonical_bytes = values
        .encode_canonical_json(&manifest)
        .ok_or(ForecastTrainingPackageVerificationError::ManifestCanonical)?;

    // require exact canonical bytes rather than only an equivalent decoded value
    if canonical_bytes != manifest_bytes {
        return Err(ForecastTrainingPackageVerificationError::ManifestCanonical);
    }

    let admitted = admit_forecast_training_package_manifest(values, &manifest)
        .ok_or(ForecastTrainingPackageVerificationError::ManifestBoundary)?;
    verify_admitted_package(&absolute_root, &admitted, values)?;
    Ok(manifest_digest.as_str().to_owned())
}

/// Read one required regular control file without collapsing effect failures.
fn read_control_file(
    values: &mut impl ForecastTrainingPackageVerificationValues,
    root: &str,
    path: &str,
) -> Result<Vec<u8>, ForecastTrainingPackageVerificationError> {
    values
        .read_regular_file(root, path)
        .map_err(effect)?
        .ok_or(ForecastTrainingPackageVerificationError::ControlFiles)
}

#[derive(Debug, Default)]
struct VerifiedMetricDates {
    relative_humidity_percent: BTreeSet<String>,
    temperature_c: BTreeSet<String>,
    wind_direction_degrees: BTreeSet<String>,
    wind_gust_mps: BTreeSet<String>,
    wind_speed_mps: BTreeSet<String>,
}

/// Verify every declared member plus the global grid, lineage, coverage, and tree proofs.
fn verify_admitted_package(
    root: &str,
    manifest: &AdmittedForecastTrainingPackageManifest,
    values: &mut impl ForecastTrainingPackageVerificationValues,
) -> Result<(), ForecastTrainingPackageVerificationError> {
    let dates = values
        .inclusive_local_dates(&manifest.from_local_date, &manifest.to_local_date)
        .ok_or(ForecastTrainingPackageVerificationError::ManifestBoundary)?;

    // rebind the admitted inclusive count to the calendar owner's exact labels
    if dates.len() as u64 != manifest.inclusive_date_count {
        return Err(ForecastTrainingPackageVerificationError::ManifestBoundary);
    }

    let mut verified_rows = 0_u64;
    let mut observed_sources =
        BTreeMap::<(&'static str, &'static str), ForecastTrainingPackageSourceIdentity>::new();
    let mut station_grid = BTreeMap::<String, Vec<i64>>::new();
    let mut station_coverage = ForecastObservationStationKey::WIRE_VALUES
        .iter()
        .map(|(wire, _)| ((*wire).to_owned(), VerifiedMetricDates::default()))
        .collect::<BTreeMap<_, _>>();
    let mut expected_entries =
        BTreeSet::from([MANIFEST_PATH.to_owned(), MANIFEST_CHECKSUM_PATH.to_owned()]);

    // verify every compressed member in manifest order
    for member in &manifest.members {
        // require the closed basename component before checking the complete path grammar
        if !valid_member_leaf(&member.path) {
            return Err(ForecastTrainingPackageVerificationError::MemberLeaf);
        }
        // reject malformed member paths before any filesystem operation
        if !valid_member_path(&member.path) {
            return Err(ForecastTrainingPackageVerificationError::MemberPath);
        }

        let compressed = values
            .read_regular_file(root, &member.path)
            .map_err(effect)?
            .ok_or(ForecastTrainingPackageVerificationError::MemberMissing)?;
        let observed_hash = values
            .sha256(&compressed)
            .ok_or(ForecastTrainingPackageVerificationError::MemberChecksum)?;

        // bind exact compressed bytes and length to the manifest member claim
        if observed_hash != member.sha256 || compressed.len() as u64 != member.size_bytes {
            return Err(ForecastTrainingPackageVerificationError::MemberChecksum);
        }

        let inflated = values.gunzip(&compressed).map_err(effect)?;
        let plaintext = String::from_utf8_lossy(&inflated);
        let lines = member_lines(&plaintext);

        // require a nonempty exact row count and Node-compatible reencoded UTF-8 length
        if lines.is_empty()
            || lines.len() as u64 != member.row_count
            || plaintext.len() as u64 != member.plaintext_bytes
        {
            return Err(ForecastTrainingPackageVerificationError::MemberPlaintext);
        }

        // preserve JavaScript's default UTF-16 code-unit row ordering
        if lines
            .windows(2)
            .any(|pair| js_string_cmp(pair[0], pair[1]).is_gt())
        {
            return Err(ForecastTrainingPackageVerificationError::MemberOrder);
        }

        let mut minimum: Option<String> = None;
        let mut maximum: Option<String> = None;
        let mut partition: Option<(String, ForecastTrainingRecordKind, Option<String>)> = None;
        let mut station_instants = Vec::new();

        // validate and aggregate every exact JSONL row
        for line in lines {
            let row = values
                .decode_json(line.as_bytes())
                .ok_or(ForecastTrainingPackageVerificationError::RowJson)?;
            let validated = validate_forecast_training_package_row(
                values,
                row,
                &manifest.from_local_date,
                &manifest.to_local_date,
            )
            .map_err(ForecastTrainingPackageVerificationError::Row)?;

            // bind each validated row to its sole deterministic member partition
            if validated.member_path != member.path {
                return Err(ForecastTrainingPackageVerificationError::RowPartition);
            }

            record_metric_dates(&mut station_coverage, &validated)?;
            partition.get_or_insert_with(|| {
                (
                    validated.local_date.clone(),
                    validated.record_kind,
                    validated.station_key.clone(),
                )
            });

            // retain station-hour instants for the later complete-grid proof
            if validated.record_kind == ForecastTrainingRecordKind::StationHour {
                station_instants.push(validated.valid_at_epoch_ms);
            }

            minimum = Some(match minimum {
                Some(current) if js_string_cmp(&current, &validated.valid_at).is_lt() => current,
                _ => validated.valid_at.clone(),
            });
            maximum = Some(match maximum {
                Some(current) if js_string_cmp(&current, &validated.valid_at).is_gt() => current,
                _ => validated.valid_at.clone(),
            });

            // preserve Map replacement semantics for every aligned observed source identity
            for identity in validated.source_identities {
                observed_sources.insert(
                    (identity.source_key, identity.source_config_fingerprint),
                    identity,
                );
            }

            verified_rows = verified_rows
                .checked_add(1)
                .ok_or(ForecastTrainingPackageVerificationError::TotalRows)?;
        }

        let Some((local_date, record_kind, station_key)) = partition else {
            return Err(ForecastTrainingPackageVerificationError::MemberPlaintext);
        };

        // reject any false date, kind, station, minimum, or maximum member claim
        if minimum.as_deref() != Some(member.minimum_valid_at.as_str())
            || maximum.as_deref() != Some(member.maximum_valid_at.as_str())
            || local_date != member.local_date
            || record_kind != member.record_kind
            || station_key != member.station_key
        {
            return Err(ForecastTrainingPackageVerificationError::MemberBounds);
        }

        // retain only physical-station members for the later global grid proof
        if record_kind == ForecastTrainingRecordKind::StationHour {
            station_grid.insert(member.path.clone(), station_instants);
        }

        add_member_inventory(&mut expected_entries, &member.path);
    }

    // bind the package total to the exact number of validated member rows
    if verified_rows != manifest.total_row_count {
        return Err(ForecastTrainingPackageVerificationError::TotalRows);
    }

    verify_station_grid(&dates, &station_grid, values)?;

    let observed_source_list = observed_sources.into_values().collect::<Vec<_>>();

    // bind every observed lineage tuple and its canonical source-key order
    if observed_source_list != manifest.observed_source_identities {
        return Err(ForecastTrainingPackageVerificationError::SourceIdentities);
    }

    verify_station_coverage(manifest, &station_coverage)?;
    let mut actual_entries = values
        .collect_regular_tree_entries(root)
        .map_err(effect)?
        .ok_or(ForecastTrainingPackageVerificationError::Inventory)?;
    actual_entries.sort();

    // reject every undeclared, missing, symbolic, special, or differently spelled entry
    if actual_entries != expected_entries.into_iter().collect::<Vec<_>>() {
        return Err(ForecastTrainingPackageVerificationError::Inventory);
    }

    Ok(())
}

/// Record non-null physical-station metrics at most once per local date.
fn record_metric_dates(
    coverage: &mut BTreeMap<String, VerifiedMetricDates>,
    row: &crate::forecast_training_package_row::ValidatedForecastTrainingPackageRow,
) -> Result<(), ForecastTrainingPackageVerificationError> {
    // skip forecast rows exactly as the predecessor coverage proof did
    if row.record_kind != ForecastTrainingRecordKind::StationHour {
        return Ok(());
    }

    let station_key = row
        .station_key
        .as_ref()
        .ok_or(ForecastTrainingPackageVerificationError::StationCoverage)?;
    let station = coverage
        .get_mut(station_key)
        .ok_or(ForecastTrainingPackageVerificationError::StationCoverage)?;
    record_present_metric_dates(station, &row.local_date, &row.metrics);
    Ok(())
}

/// Add the row date to each present metric's exact-date set.
fn record_present_metric_dates(
    coverage: &mut VerifiedMetricDates,
    local_date: &str,
    metrics: &ForecastTrainingRowMetrics,
) {
    // retain one conservative non-null date upper bound per metric
    if metrics.relative_humidity_percent.is_some() {
        coverage
            .relative_humidity_percent
            .insert(local_date.to_owned());
    }
    // retain one conservative non-null date upper bound per metric
    if metrics.temperature_c.is_some() {
        coverage.temperature_c.insert(local_date.to_owned());
    }
    // retain one conservative non-null date upper bound per metric
    if metrics.wind_direction_degrees.is_some() {
        coverage
            .wind_direction_degrees
            .insert(local_date.to_owned());
    }
    // retain one conservative non-null date upper bound per metric
    if metrics.wind_gust_mps.is_some() {
        coverage.wind_gust_mps.insert(local_date.to_owned());
    }
    // retain one conservative non-null date upper bound per metric
    if metrics.wind_speed_mps.is_some() {
        coverage.wind_speed_mps.insert(local_date.to_owned());
    }
}

/// Prove every admitted date has one unique exact hourly row for every physical station.
fn verify_station_grid(
    dates: &[String],
    station_grid: &BTreeMap<String, Vec<i64>>,
    values: &mut impl ForecastTrainingPackageVerificationValues,
) -> Result<(), ForecastTrainingPackageVerificationError> {
    // verify every inclusive local date independently for DST transitions
    for date in dates {
        let expected = values
            .local_date_utc_hours(date)
            .ok_or(ForecastTrainingPackageVerificationError::StationGrid)?;
        verify_date_station_grid(date, &expected, station_grid)?;
    }

    Ok(())
}

/// Prove all eleven station members equal one calendar-owned local-day UTC grid.
fn verify_date_station_grid(
    date: &str,
    expected: &[i64],
    station_grid: &BTreeMap<String, Vec<i64>>,
) -> Result<(), ForecastTrainingPackageVerificationError> {
    // require all eleven physical stations on this exact date
    for (station_key, _) in ForecastObservationStationKey::WIRE_VALUES {
        let path = format!("members/{date}/station-hour/{station_key}.jsonl.gz");
        let mut actual = station_grid
            .get(&path)
            .cloned()
            .ok_or(ForecastTrainingPackageVerificationError::StationGrid)?;
        let unique = actual.iter().copied().collect::<BTreeSet<_>>();
        actual.sort_unstable();

        // reject missing, duplicate, shifted, or calendar-owner-disordered hours
        if unique.len() != actual.len() || actual != expected {
            return Err(ForecastTrainingPackageVerificationError::StationGrid);
        }
    }

    Ok(())
}

/// Bind typed manifest coverage claims to the exact verified row-derived date sets.
fn verify_station_coverage(
    manifest: &AdmittedForecastTrainingPackageManifest,
    actual: &BTreeMap<String, VerifiedMetricDates>,
) -> Result<(), ForecastTrainingPackageVerificationError> {
    // compare every typed station claim in its already admitted canonical order
    for claimed in &manifest.station_metric_coverage {
        let station_key = claimed.station_key_wire();
        let verified = actual
            .get(station_key)
            .ok_or(ForecastTrainingPackageVerificationError::StationCoverage)?;
        let counts = &claimed.eligible_metric_non_null_local_dates;

        // require every metric's distinct local-date count to match exactly
        if counts.relative_humidity_percent != verified.relative_humidity_percent.len() as u64
            || counts.temperature_c != verified.temperature_c.len() as u64
            || counts.wind_direction_degrees != verified.wind_direction_degrees.len() as u64
            || counts.wind_gust_mps != verified.wind_gust_mps.len() as u64
            || counts.wind_speed_mps != verified.wind_speed_mps.len() as u64
        {
            return Err(ForecastTrainingPackageVerificationError::StationCoverage);
        }
    }

    Ok(())
}

/// Add one member plus every parent directory spelling to the expected tree.
fn add_member_inventory(entries: &mut BTreeSet<String>, member_path: &str) {
    entries.insert(member_path.to_owned());
    let mut parts = member_path.split('/').collect::<Vec<_>>();
    parts.pop();

    // bind each declared member's complete directory ancestry
    while !parts.is_empty() {
        entries.insert(format!("{}/", parts.join("/")));
        parts.pop();
    }
}

/// Recognize the one closed ASCII member path grammar without adding a regex package.
fn valid_member_path(path: &str) -> bool {
    let parts = path.split('/').collect::<Vec<_>>();

    // require the fixed root, date, kind, and leaf segments with no traversal spelling
    if path.contains("..")
        || parts.len() != 4
        || parts[0] != "members"
        || !valid_date_label(parts[1])
        || !matches!(
            parts[2],
            "station-hour" | "fixed-lead-anchor" | "legacy-v4-retrieval"
        )
    {
        return false;
    }

    parts[3].strip_suffix(".jsonl.gz").is_some()
}

/// Recognize the predecessor's separately diagnosed member leaf component.
fn valid_member_leaf(path: &str) -> bool {
    path.rsplit('/')
        .next()
        .is_some_and(|leaf| valid_safe_component(leaf.strip_suffix(".jsonl.gz").unwrap_or(leaf)))
}

/// Recognize the predecessor's exact four-digit, two-digit, two-digit date shape.
fn valid_date_label(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
}

/// Recognize the original lowercase safe-component grammar and 128-byte bound.
fn valid_safe_component(value: &str) -> bool {
    let bytes = value.as_bytes();

    // require one lowercase alphanumeric prefix and only the frozen ASCII tail alphabet
    if bytes.is_empty()
        || bytes.len() > 128
        || !(bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
    {
        return false;
    }
    bytes.iter().skip(1).all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
    })
}

/// Split one Node-decoded member using JavaScript trimEnd whitespace semantics.
fn member_lines(plaintext: &str) -> Vec<&str> {
    // preserve the predecessor's special empty-file result
    if plaintext.is_empty() {
        return Vec::new();
    }
    plaintext
        .trim_end_matches(js_whitespace)
        .split('\n')
        .collect()
}

/// Recognize JavaScript WhiteSpace and LineTerminator code points used by trimEnd.
fn js_whitespace(value: char) -> bool {
    matches!(
        value,
        '\u{0009}'
            | '\u{000A}'
            | '\u{000B}'
            | '\u{000C}'
            | '\u{000D}'
            | '\u{0020}'
            | '\u{00A0}'
            | '\u{1680}'
            | '\u{2000}'
            ..='\u{200A}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202F}'
                | '\u{205F}'
                | '\u{3000}'
                | '\u{FEFF}'
    )
}

/// Compare valid Rust strings by JavaScript's default UTF-16 code-unit ordering.
fn js_string_cmp(left: &str, right: &str) -> std::cmp::Ordering {
    left.encode_utf16().cmp(right.encode_utf16())
}

/// Preserve one approved primitive failure without inventing a new classification.
fn effect(error: impl Display) -> ForecastTrainingPackageVerificationError {
    ForecastTrainingPackageVerificationError::Effect(error.to_string())
}

#[cfg(test)]
#[path = "forecast_training_package_full_fixture_test.rs"]
pub(crate) mod full_fixture_tests;

#[cfg(test)]
mod tests {
    use super::*;

    /// Preserve path, inventory, JavaScript ordering, and trim semantics at the pure boundary.
    #[test]
    fn recognizes_exact_member_and_text_contracts() {
        assert!(valid_member_path(
            "members/2026-11-01/station-hour/tempest-64255.jsonl.gz"
        ));
        assert_eq!(member_lines("a\nb\n\u{feff}"), vec!["a", "b"]);
        assert!(js_string_cmp("\u{e000}", "\u{10000}").is_gt());

        let mut inventory = BTreeSet::new();
        add_member_inventory(
            &mut inventory,
            "members/2026-11-01/station-hour/tempest-64255.jsonl.gz",
        );
        assert_eq!(
            inventory.into_iter().collect::<Vec<_>>(),
            vec![
                "members/".to_owned(),
                "members/2026-11-01/".to_owned(),
                "members/2026-11-01/station-hour/".to_owned(),
                "members/2026-11-01/station-hour/tempest-64255.jsonl.gz".to_owned(),
            ]
        );
    }

    /// Accept one complete ordinary 24-hour day and reject one duplicate station hour.
    #[test]
    fn proves_complete_eleven_station_grid() {
        let date = "2026-11-02";
        let expected = (0_i64..24).collect::<Vec<_>>();
        let mut grid = ForecastObservationStationKey::WIRE_VALUES
            .iter()
            .map(|(station, _)| {
                (
                    format!("members/{date}/station-hour/{station}.jsonl.gz"),
                    expected.clone(),
                )
            })
            .collect::<BTreeMap<_, _>>();

        assert_eq!(verify_date_station_grid(date, &expected, &grid), Ok(()));
        let first_path = format!(
            "members/{date}/station-hour/{}.jsonl.gz",
            ForecastObservationStationKey::WIRE_VALUES[0].0
        );
        grid.get_mut(&first_path)
            .expect("complete fixture station should exist")
            .push(23);
        assert_eq!(
            verify_date_station_grid(date, &expected, &grid),
            Err(ForecastTrainingPackageVerificationError::StationGrid)
        );

        let fallback_date = "2026-11-01";
        let fallback_expected = (0_i64..25).collect::<Vec<_>>();
        let fallback_grid = ForecastObservationStationKey::WIRE_VALUES
            .iter()
            .map(|(station, _)| {
                (
                    format!("members/{fallback_date}/station-hour/{station}.jsonl.gz"),
                    fallback_expected.clone(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        assert_eq!(
            verify_date_station_grid(fallback_date, &fallback_expected, &fallback_grid),
            Ok(())
        );
    }
}
