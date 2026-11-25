//! Admit one verifier-bound forecast and adjustment package pair for native rain scoring.

use std::fmt::{self, Display};

use weather_contracts::JsonValue;
use weather_worker::RainPerformanceReportPackageInputs;

use crate::maintenance_canonical_json::{field, text};
use crate::{
    AdjustmentEvaluationPackageVerificationValues, AdmittedRainArchiveBody,
    AdmittedRainPerformancePackagePair, ForecastTrainingPackageVerificationValues,
    RainPerformanceAdjustmentRowsError, RainPerformanceEdgeRowsError,
    RainPerformanceEdgeRowsValues, RainPerformanceTimestampValues,
    VerifiedRainPerformanceEdgeMember, admit_verified_rain_performance_edge_rows,
    load_verified_adjustment_evaluation_package,
    normalize_admitted_rain_performance_adjustment_rows,
    normalize_verified_performance_timestamp_field, normalize_verified_performance_timestamps,
    verify_forecast_training_package,
};

const FORECAST_MANIFEST_PATH: &str = "manifest.json";
const SITE_KEY: &str = "ballydidean";

/// Opaque adjustment material returned only after its existing package verifier succeeds.
#[derive(Debug, Clone, PartialEq)]
pub struct VerifiedRainPerformanceAdjustmentPackage {
    pub bodies: Vec<AdmittedRainArchiveBody>,
    pub edge_members: Vec<VerifiedRainPerformanceEdgeMember>,
    pub manifest: JsonValue,
    pub manifest_sha256: String,
    pub rows: Vec<JsonValue>,
}

/// approved path, file, JSON, compression, timestamp, and digest owners
pub trait RainPerformancePackageLoaderValues:
    RainPerformanceEdgeRowsValues + RainPerformanceTimestampValues
{
    type Error: Display;

    /// resolve one caller-supplied package path with the original process path owner
    fn resolve_package_root(&mut self, raw: &str) -> Result<String, Self::Error>;

    /// read one exact member relative to its already resolved package root
    fn read_package_file(&mut self, root: &str, member_path: &str) -> Result<Vec<u8>, Self::Error>;

    /// inflate one verifier-declared forecast member with the approved gzip owner
    fn gunzip_forecast_member(&mut self, compressed: &[u8]) -> Result<Vec<u8>, Self::Error>;
}

/// retained opaque hooks used only by the predecessor-compatible loader entry point
pub trait RainPerformanceLegacyPackageLoaderValues: RainPerformancePackageLoaderValues {
    /// run the retained forecast verifier and return its sole manifest digest
    fn verify_forecast_package(&mut self, root: &str) -> Result<String, Self::Error>;

    /// load one package through the retained adjustment verifier
    fn load_verified_adjustment_package(
        &mut self,
        root: &str,
    ) -> Result<VerifiedRainPerformanceAdjustmentPackage, Self::Error>;
}

/// the three independent owner groups used by the fully native package path
pub struct RainPerformanceNativePackageValues<'a, PackageValues, ForecastValues, AdjustmentValues> {
    package_values: &'a mut PackageValues,
    forecast_values: &'a mut ForecastValues,
    adjustment_values: &'a mut AdjustmentValues,
}

impl<'a, PackageValues, ForecastValues, AdjustmentValues>
    RainPerformanceNativePackageValues<'a, PackageValues, ForecastValues, AdjustmentValues>
{
    /// bind all native package owners for one command invocation
    pub fn new(
        package_values: &'a mut PackageValues,
        forecast_values: &'a mut ForecastValues,
        adjustment_values: &'a mut AdjustmentValues,
    ) -> Self {
        Self {
            package_values,
            forecast_values,
            adjustment_values,
        }
    }
}

/// Closed failures from matched package admission.
#[derive(Debug, PartialEq)]
pub enum RainPerformancePackageLoaderError {
    AdjustmentManifest,
    AdjustmentRows(RainPerformanceAdjustmentRowsError),
    AdjustmentWatermark,
    CohortMismatch,
    EdgeRows(RainPerformanceEdgeRowsError),
    Effect(String),
    ForecastManifest,
    ForecastManifestChanged,
    ForecastMemberChanged,
    ForecastMemberHash,
    ForecastMemberObject,
    ForecastMemberPath,
    ForecastMemberSize,
    ForecastMembersUnavailable,
    ForecastRow,
    ForecastTargetCutoff,
    ForecastTimestamp(RainPerformanceAdjustmentRowsError),
}

impl Display for RainPerformancePackageLoaderError {
    /// Preserve the original package-loader diagnostics at the command boundary.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // retain exact predecessor messages for every ordinary failure
        match self {
            Self::AdjustmentManifest => {
                formatter.write_str("adjustment package manifest must be an object")
            }
            Self::AdjustmentRows(error) | Self::ForecastTimestamp(error) => {
                Display::fmt(error, formatter)
            }
            Self::AdjustmentWatermark => {
                formatter.write_str("adjustment watermark must be a nonempty string")
            }
            Self::CohortMismatch => {
                formatter.write_str("forecast and adjustment packages cover different cohorts")
            }
            Self::EdgeRows(error) => Display::fmt(error, formatter),
            Self::Effect(message) => formatter.write_str(message),
            Self::ForecastManifest => {
                formatter.write_str("forecast package manifest must be an object")
            }
            Self::ForecastManifestChanged => {
                formatter.write_str("forecast package changed after verification")
            }
            Self::ForecastMemberChanged => {
                formatter.write_str("forecast package member changed after verification")
            }
            Self::ForecastMemberHash => {
                formatter.write_str("forecast package member hash must be a nonempty string")
            }
            Self::ForecastMemberObject => {
                formatter.write_str("forecast package member must be an object")
            }
            Self::ForecastMemberPath => {
                formatter.write_str("forecast package member path must be a nonempty string")
            }
            Self::ForecastMemberSize => {
                formatter.write_str("forecast package member size must be finite")
            }
            Self::ForecastMembersUnavailable => {
                formatter.write_str("forecast package members are unavailable")
            }
            Self::ForecastRow => formatter.write_str("forecast package row must be an object"),
            Self::ForecastTargetCutoff => {
                formatter.write_str("forecast target cutoff must be a nonempty string")
            }
        }
    }
}

impl std::error::Error for RainPerformancePackageLoaderError {}

/// verify, match, and project one package pair without retaining unused forecast rows
pub fn load_verified_rain_performance_package_pair(
    forecast_package: &str,
    adjustment_package: &str,
    values: &mut impl RainPerformanceLegacyPackageLoaderValues,
) -> Result<AdmittedRainPerformancePackagePair, RainPerformancePackageLoaderError> {
    load_verified_rain_performance_package_pair_with(
        forecast_package,
        adjustment_package,
        values,
        |values, root| values.verify_forecast_package(root),
        |values, root| values.load_verified_adjustment_package(root),
        |_| {},
    )
}

/// verify both packages with their native domain verifiers before admitting the rain pair
pub fn load_verified_rain_performance_package_pair_with_native_verifiers<
    PackageValues,
    ForecastValues,
    AdjustmentValues,
>(
    forecast_package: &str,
    adjustment_package: &str,
    values: &mut RainPerformanceNativePackageValues<
        '_,
        PackageValues,
        ForecastValues,
        AdjustmentValues,
    >,
) -> Result<AdmittedRainPerformancePackagePair, RainPerformancePackageLoaderError>
where
    PackageValues: RainPerformancePackageLoaderValues,
    ForecastValues: ForecastTrainingPackageVerificationValues,
    AdjustmentValues: AdjustmentEvaluationPackageVerificationValues,
{
    load_native_performance_pair_with_forecast_rows(
        forecast_package,
        adjustment_package,
        values,
        |_| {},
    )
}

/// stream verified forecast rows to the family that consumes them
pub(crate) fn load_native_performance_pair_with_forecast_rows<
    PackageValues,
    ForecastValues,
    AdjustmentValues,
>(
    forecast_package: &str,
    adjustment_package: &str,
    values: &mut RainPerformanceNativePackageValues<
        '_,
        PackageValues,
        ForecastValues,
        AdjustmentValues,
    >,
    collect_forecast_row: impl FnMut(JsonValue),
) -> Result<AdmittedRainPerformancePackagePair, RainPerformancePackageLoaderError>
where
    PackageValues: RainPerformancePackageLoaderValues,
    ForecastValues: ForecastTrainingPackageVerificationValues,
    AdjustmentValues: AdjustmentEvaluationPackageVerificationValues,
{
    let RainPerformanceNativePackageValues {
        package_values,
        forecast_values,
        adjustment_values,
    } = values;
    let package_values = &mut **package_values;
    let forecast_values = &mut **forecast_values;
    let adjustment_values = &mut **adjustment_values;
    load_verified_rain_performance_package_pair_with(
        forecast_package,
        adjustment_package,
        package_values,
        |_, root| verify_forecast_training_package(root, &mut *forecast_values),
        |_, root| load_verified_adjustment_evaluation_package(root, &mut *adjustment_values),
        collect_forecast_row,
    )
}

/// preserve the package effect order while selecting both exact verifier owners
fn load_verified_rain_performance_package_pair_with<Values, ForecastError, AdjustmentError>(
    forecast_package: &str,
    adjustment_package: &str,
    values: &mut Values,
    mut verify_forecast_package: impl FnMut(&mut Values, &str) -> Result<String, ForecastError>,
    mut load_adjustment_package: impl FnMut(
        &mut Values,
        &str,
    ) -> Result<
        VerifiedRainPerformanceAdjustmentPackage,
        AdjustmentError,
    >,
    mut collect_forecast_row: impl FnMut(JsonValue),
) -> Result<AdmittedRainPerformancePackagePair, RainPerformancePackageLoaderError>
where
    Values: RainPerformancePackageLoaderValues,
    ForecastError: Display,
    AdjustmentError: Display,
{
    let forecast_root = values
        .resolve_package_root(forecast_package)
        .map_err(effect)?;
    let adjustment_root = values
        .resolve_package_root(adjustment_package)
        .map_err(effect)?;
    let forecast_manifest_bytes = values
        .read_package_file(&forecast_root, FORECAST_MANIFEST_PATH)
        .map_err(effect)?;
    let forecast_manifest_sha256 =
        verify_forecast_package(values, &forecast_root).map_err(effect)?;
    let observed_manifest_sha256 = values
        .sha256(&forecast_manifest_bytes)
        .ok_or(RainPerformancePackageLoaderError::ForecastManifestChanged)?;

    // bind the verifier result to the bytes snapshotted before verification
    if observed_manifest_sha256.as_str() != forecast_manifest_sha256 {
        return Err(RainPerformancePackageLoaderError::ForecastManifestChanged);
    }

    let forecast_manifest = values
        .decode_json(&forecast_manifest_bytes)
        .filter(is_object)
        .ok_or(RainPerformancePackageLoaderError::ForecastManifest)?;
    let adjustment = load_adjustment_package(values, &adjustment_root).map_err(effect)?;

    // reject cross-site or cross-date pairs before opening either scored population
    let cohort = matched_cohort(&forecast_manifest, &adjustment.manifest)?;
    validate_adjustment_edge_evidence(&adjustment.manifest)?;

    let normalized_adjustment_rows =
        normalize_admitted_rain_performance_adjustment_rows(adjustment.rows, values)
            .map_err(RainPerformancePackageLoaderError::AdjustmentRows)?;
    let edge_rows = admit_verified_rain_performance_edge_rows(adjustment.edge_members, values)
        .map_err(RainPerformancePackageLoaderError::EdgeRows)?;

    // rebind and decode every forecast member before passing rows to the scoring family
    verify_forecast_members(
        &forecast_root,
        &forecast_manifest,
        values,
        &mut collect_forecast_row,
    )?;
    let watermark_sha256 = adjustment_watermark(&adjustment.manifest)?;
    let created_at = required_text(&forecast_manifest, "createdAtUtc")
        .ok_or(RainPerformancePackageLoaderError::ForecastTargetCutoff)?;
    let target_cutoff_at = normalize_verified_performance_timestamp_field(
        &"createdAtUtc".encode_utf16().collect::<Vec<_>>(),
        created_at.encode_utf16().collect(),
        values,
    )
    .map_err(RainPerformancePackageLoaderError::ForecastTimestamp)
    .and_then(|value| {
        String::from_utf16(&value).map_err(|_| RainPerformancePackageLoaderError::ForecastManifest)
    })?;

    Ok(AdmittedRainPerformancePackagePair {
        inputs: RainPerformanceReportPackageInputs {
            adjustment_evidence_manifest_sha256: adjustment.manifest_sha256,
            adjustment_evidence_watermark_sha256: watermark_sha256,
            forecast_training_manifest_sha256: forecast_manifest_sha256,
            local_date_from: cohort.0,
            local_date_to: cohort.1,
            target_cutoff_at,
        },
        bodies: adjustment.bodies,
        claims: Vec::new(),
        edge_rows,
        normalized_adjustment_rows,
        receipts: Vec::new(),
        runs: Vec::new(),
    })
}

/// Retain the one exact shared package interval after checking the fixed site.
fn matched_cohort(
    forecast: &JsonValue,
    adjustment: &JsonValue,
) -> Result<(String, String), RainPerformancePackageLoaderError> {
    let forecast_site = required_text(forecast, "siteKey");
    let adjustment_site = required_text(adjustment, "siteKey");
    let forecast_from = required_text(forecast, "fromLocalDate");
    let adjustment_from = required_text(adjustment, "fromLocalDate");
    let forecast_to = required_text(forecast, "toLocalDate");
    let adjustment_to = required_text(adjustment, "toLocalDate");

    // preserve the predecessor's one fixed site and exact endpoint comparisons
    if forecast_site.as_deref() != Some(SITE_KEY)
        || adjustment_site.as_deref() != Some(SITE_KEY)
        || forecast_from != adjustment_from
        || forecast_to != adjustment_to
    {
        return Err(RainPerformancePackageLoaderError::CohortMismatch);
    }

    match (forecast_from, forecast_to) {
        (Some(from), Some(to)) => Ok((from, to)),
        _ => Err(RainPerformancePackageLoaderError::CohortMismatch),
    }
}

/// Validate the adjustment edge evidence object before opening returned row populations.
fn validate_adjustment_edge_evidence(
    manifest: &JsonValue,
) -> Result<(), RainPerformancePackageLoaderError> {
    field(manifest, "edgeEvidence")
        .filter(|value| is_object(value))
        .map(|_| ())
        .ok_or(RainPerformancePackageLoaderError::AdjustmentManifest)
}

/// Read the verifier-bound adjustment watermark used in report inputs.
fn adjustment_watermark(manifest: &JsonValue) -> Result<String, RainPerformancePackageLoaderError> {
    let edge_evidence = field(manifest, "edgeEvidence")
        .filter(|value| is_object(value))
        .ok_or(RainPerformancePackageLoaderError::AdjustmentManifest)?;
    required_text(edge_evidence, "watermarkSha256")
        .ok_or(RainPerformancePackageLoaderError::AdjustmentWatermark)
}

/// Re-read every declared member and validate/decode its rows one at a time.
fn verify_forecast_members(
    root: &str,
    manifest: &JsonValue,
    values: &mut impl RainPerformancePackageLoaderValues,
    collect_forecast_row: &mut impl FnMut(JsonValue),
) -> Result<(), RainPerformancePackageLoaderError> {
    let JsonValue::Array(members) = field(manifest, "members")
        .ok_or(RainPerformancePackageLoaderError::ForecastMembersUnavailable)?
    else {
        return Err(RainPerformancePackageLoaderError::ForecastMembersUnavailable);
    };

    // preserve verifier-declared member and row order without retaining decoded rows
    for member in members {
        // require the same plain descriptor object before reading its fields
        if !is_object(member) {
            return Err(RainPerformancePackageLoaderError::ForecastMemberObject);
        }
        let path = required_text(member, "path")
            .ok_or(RainPerformancePackageLoaderError::ForecastMemberPath)?;
        let expected_hash = required_text(member, "sha256")
            .ok_or(RainPerformancePackageLoaderError::ForecastMemberHash)?;
        let expected_size = finite_number(member, "sizeBytes")
            .ok_or(RainPerformancePackageLoaderError::ForecastMemberSize)?;
        let compressed = values.read_package_file(root, &path).map_err(effect)?;
        let observed_hash = values
            .sha256(&compressed)
            .ok_or(RainPerformancePackageLoaderError::ForecastMemberChanged)?;

        // reject either byte-length or content drift before decompression
        if compressed.len() as f64 != expected_size || observed_hash.as_str() != expected_hash {
            return Err(RainPerformancePackageLoaderError::ForecastMemberChanged);
        }

        let plaintext = values.gunzip_forecast_member(&compressed).map_err(effect)?;
        verify_forecast_rows(&plaintext, values, collect_forecast_row)?;
    }
    Ok(())
}

/// decode and normalize each nonempty jsonl row before transferring ownership
fn verify_forecast_rows(
    plaintext: &[u8],
    values: &mut impl RainPerformancePackageLoaderValues,
    collect_forecast_row: &mut impl FnMut(JsonValue),
) -> Result<(), RainPerformancePackageLoaderError> {
    let plaintext = std::str::from_utf8(plaintext)
        .map_err(|_| RainPerformancePackageLoaderError::ForecastRow)?;
    let plaintext = plaintext.trim_end();

    // decode the original nonempty line population in order
    for line in plaintext.split('\n') {
        // preserve the verifier-proven nonempty member invariant
        if line.is_empty() {
            continue;
        }
        let row = values
            .decode_json(line.as_bytes())
            .ok_or(RainPerformancePackageLoaderError::ForecastRow)?;
        let row = normalize_verified_performance_timestamps(row, values)
            .map_err(RainPerformancePackageLoaderError::ForecastTimestamp)?;
        // require the same plain object shape after recursive normalization
        if !is_object(&row) {
            return Err(RainPerformancePackageLoaderError::ForecastRow);
        }
        collect_forecast_row(row);
    }
    Ok(())
}

/// Convert any effect failure into its original printable diagnostic.
fn effect(error: impl Display) -> RainPerformancePackageLoaderError {
    RainPerformancePackageLoaderError::Effect(error.to_string())
}

/// Borrow one nonempty string field with the predecessor's text contract.
fn required_text(value: &JsonValue, key: &str) -> Option<String> {
    text(value, key).filter(|value| !value.is_empty())
}

/// Borrow one finite numeric field for exact JavaScript number comparison.
fn finite_number(value: &JsonValue, key: &str) -> Option<f64> {
    let JsonValue::Number(value) = field(value, key)? else {
        return None;
    };
    value.is_finite().then_some(*value)
}

/// Recognize the plain object shape shared by manifest and row decoding.
fn is_object(value: &JsonValue) -> bool {
    matches!(value, JsonValue::Object(_))
}

#[cfg(test)]
#[path = "rain_performance_package_loader_test.rs"]
mod tests;
