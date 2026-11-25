//! File-only scorecard assembly and exact-byte verification commands.

use std::cmp::Ordering;
use std::fmt::{self, Display};
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;

use weather_contracts::{ForecastAdjustmentPerformancePackageInputs, JsonValue};

use crate::forecast_scorecard_contract::{SEVEN_DAYS_MS, is_wire, wire};
use crate::forecast_scorecard_fields::{member, require_object};
use crate::maintenance_canonical_json::{field, text};
use crate::rain_performance_cli::{ParsedOptions, one_option, option_values, parse_options};
use crate::{
    FORECAST_SCORECARD_V1, MaintenanceCanonicalJson, NativeForecastTrainingPackageFiles,
    RainPerformanceCliError, RainPerformanceResultWriter, RainPerformanceTimestampValues,
    ScorecardContractError, ScorecardContractValues,
    normalize_verified_performance_timestamp_field, parse_forecast_adjustment_scorecard,
    validate_forecast_adjustment_scorecard,
};

const FAMILY_REPORT_VERSION: &str = "forecast-adjustment-performance-report/v1";

/// Read scorecard inputs relative to one captured working directory.
pub struct NativeScorecardFiles(NativeForecastTrainingPackageFiles);

impl NativeScorecardFiles {
    /// bind the original lexical argument root
    pub fn new(working_directory: PathBuf) -> io::Result<Self> {
        NativeForecastTrainingPackageFiles::new(working_directory).map(Self)
    }

    /// read one caller-selected report, manifest, or scorecard unchanged
    pub fn read(&self, raw: &str) -> io::Result<Vec<u8>> {
        fs::read(self.0.absolute_path(raw)?)
    }
}

/// Approved clock, path, codec, digest, and ordering owners.
pub trait ScorecardCommandValues:
    ScorecardContractValues + MaintenanceCanonicalJson + RainPerformanceTimestampValues
{
    type Error: Display;

    /// capture the mapped clock once for creation or validation
    fn capture_scorecard_clock(&mut self) -> Result<i64, Self::Error>;

    /// read one exact raw input byte population
    fn read_scorecard_file(&mut self, raw: &str) -> Result<Vec<u8>, Self::Error>;

    /// order the original publication labels with the javascript locale owner
    fn compare_scorecard_labels(&self, left: &str, right: &str) -> Ordering;
}

/// Scorecard command failures with the original ordinary diagnostics.
#[derive(Debug, PartialEq)]
pub enum ScorecardCliError {
    Arguments(RainPerformanceCliError),
    Contract(ScorecardContractError),
    Effect(String),
}

impl Display for ScorecardCliError {
    /// preserve the active phase's diagnostic
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // forward the case-specific failure without publishing partial output
        match self {
            Self::Arguments(error) => Display::fmt(error, formatter),
            Self::Contract(error) => Display::fmt(error, formatter),
            Self::Effect(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for ScorecardCliError {}

/// Run scorecard creation or exact-byte verification through the native contract.
pub fn run_forecast_performance_scorecard_cli(
    argv: &[String],
    values: &mut impl ScorecardCommandValues,
    writer: &mut impl RainPerformanceResultWriter,
    stdout: &mut impl Write,
) -> Result<u8, ScorecardCliError> {
    let options = parse_options(argv).map_err(ScorecardCliError::Arguments)?;
    // dispatch before capturing the clock or opening any inputs
    match options.command.as_str() {
        "scorecard" => {
            let now = values.capture_scorecard_clock().map_err(effect)?;
            let scorecard = assemble_scorecard(&options, now, values)?;
            let path =
                one_option(&options.values, "--output").map_err(ScorecardCliError::Arguments)?;
            let target = writer.admit_result_path(path).map_err(effect)?;
            let bytes = canonical(values, &scorecard)?;
            writer.write_new_result(target, &bytes).map_err(effect)?;
        }
        "verify-scorecard" => {
            let now = values.capture_scorecard_clock().map_err(effect)?;
            let path =
                one_option(&options.values, "--input").map_err(ScorecardCliError::Arguments)?;
            let bytes = values.read_scorecard_file(path).map_err(effect)?;
            parse_forecast_adjustment_scorecard(&bytes, Some(now), values)
                .map_err(ScorecardCliError::Contract)?;
            writeln!(stdout, "{}", digest(values, &bytes)?).map_err(effect)?;
        }
        other => {
            return Err(ScorecardCliError::Arguments(
                RainPerformanceCliError::UnsupportedCommand(other.to_owned()),
            ));
        }
    }
    Ok(0)
}

struct FamilyReport {
    bytes: Vec<u8>,
    family: JsonValue,
    inputs: JsonValue,
    source_revision: JsonValue,
}

/// Bind the original family wrappers before any publication manifest is opened.
fn read_family(
    options: &ParsedOptions,
    option: &'static str,
    family: &str,
    values: &mut impl ScorecardCommandValues,
) -> Result<FamilyReport, ScorecardCliError> {
    let path = one_option(&options.values, option).map_err(ScorecardCliError::Arguments)?;
    let bytes = values.read_scorecard_file(path).map_err(effect)?;
    let document = values
        .decode_scorecard_json(&bytes)
        .map_err(ScorecardCliError::Contract)?;
    require_object(&document, &format!("{family} report")).map_err(ScorecardCliError::Contract)?;
    let card = member(&document, "family");
    require_object(card, &format!("{family} report family"))
        .map_err(ScorecardCliError::Contract)?;
    // preserve the original version, site, and named family admission
    if !is_wire(member(&document, "contractVersion"), FAMILY_REPORT_VERSION)
        || !is_wire(member(&document, "siteKey"), "ballydidean")
        || !is_wire(member(card, "family"), family)
    {
        return Err(effect(format!(
            "{family} performance report identity is invalid"
        )));
    }
    let JsonValue::Object(fields) = document else {
        unreachable!("the original family wrapper was admitted");
    };
    let mut report = FamilyReport {
        bytes,
        family: JsonValue::Null,
        inputs: JsonValue::Null,
        source_revision: JsonValue::Null,
    };
    // retain only the fields used by publication and release the decoded development payloads
    for (key, value) in fields {
        // move the admitted card without cloning its aggregate slices
        if key.iter().copied().eq("family".encode_utf16()) {
            report.family = value;
        // defer input admission until all family wrappers have been read
        } else if key.iter().copied().eq("inputs".encode_utf16()) {
            report.inputs = value;
        // defer revision binding to the same common-input comparison
        } else if key.iter().copied().eq("sourceRevision".encode_utf16()) {
            report.source_revision = value;
        }
    }
    Ok(report)
}

/// Assemble and validate the historical review-only publication.
fn assemble_scorecard(
    options: &ParsedOptions,
    now: i64,
    values: &mut impl ScorecardCommandValues,
) -> Result<JsonValue, ScorecardCliError> {
    let temperature = read_family(options, "--temperature-report", "temperature", values)?;
    let wind = read_family(options, "--wind-report", "wind", values)?;
    let rain = read_family(options, "--rain-report", "rain", values)?;
    require_object(&temperature.inputs, "family report inputs")
        .map_err(ScorecardCliError::Contract)?;
    let source_revision =
        required_wire_text(&temperature.source_revision, "family source revision")?;
    let common_bytes = canonical(values, &temperature.inputs)?;
    // check every wrapper in original family order, including the first report
    for report in [&temperature, &wind, &rain] {
        require_object(&report.inputs, "family report inputs")
            .map_err(ScorecardCliError::Contract)?;
        // reject mixed snapshots before any publication manifests are read
        if canonical(values, &report.inputs)? != common_bytes
            || !is_wire(&report.source_revision, &source_revision)
        {
            return Err(effect("family reports do not bind the same input snapshot"));
        }
    }
    let expected = publication_inputs(options, values)?;
    // require the original raw-manifest identities to be precisely represented by each report
    if canonical(values, &package_inputs_json(&expected))? != common_bytes {
        return Err(effect(
            "family reports are stale or do not match publication manifests",
        ));
    }
    let generated = values
        .canonical_utc(now)
        .ok_or_else(|| effect("generatedAt is invalid"))?;
    let JsonValue::Object(mut published_inputs) = temperature.inputs else {
        unreachable!("the common report inputs were admitted");
    };
    let report_hashes = object(vec![
        ("rain", wire(&digest_report(values, rain.bytes)?)),
        (
            "temperature",
            wire(&digest_report(values, temperature.bytes)?),
        ),
        ("wind", wire(&digest_report(values, wind.bytes)?)),
    ]);
    published_inputs.push(("reportSha256s".encode_utf16().collect(), report_hashes));
    published_inputs.push((
        "sourceRevision".encode_utf16().collect(),
        wire(&source_revision),
    ));
    let through = now
        .checked_add(SEVEN_DAYS_MS)
        .and_then(|instant| values.canonical_utc(instant))
        .ok_or_else(|| effect("validThrough is invalid"))?;
    let scorecard = object(vec![
        ("automaticActivationEligible", JsonValue::Boolean(false)),
        ("contractVersion", wire(FORECAST_SCORECARD_V1)),
        (
            "families",
            object(vec![
                ("rain", rain.family),
                ("temperature", temperature.family),
                ("wind", wind.family),
            ]),
        ),
        ("generatedAt", wire(&generated)),
        ("inputs", JsonValue::Object(published_inputs)),
        ("operatorApprovalRequired", JsonValue::Boolean(true)),
        ("servingChanged", JsonValue::Boolean(false)),
        ("siteKey", wire("ballydidean")),
        ("validThrough", wire(&through)),
    ]);
    validate_forecast_adjustment_scorecard(&scorecard, Some(now), values)
        .map_err(ScorecardCliError::Contract)?;
    Ok(scorecard)
}

/// Compose raw publication manifest identities in chronological order.
fn publication_inputs(
    options: &ParsedOptions,
    values: &mut impl ScorecardCommandValues,
) -> Result<ForecastAdjustmentPerformancePackageInputs, ScorecardCliError> {
    let forecast_paths = option_values(&options.values, "--forecast-manifest");
    let adjustment_paths = option_values(&options.values, "--adjustment-manifest");
    // require one matched adjustment manifest for each forecast manifest
    if forecast_paths.is_empty() || forecast_paths.len() != adjustment_paths.len() {
        return Err(effect(
            "publication requires matched forecast and adjustment manifests",
        ));
    }
    let mut pairs = Vec::with_capacity(forecast_paths.len());
    // verify each pair in original argument order before chronological composition
    for (forecast, adjustment) in forecast_paths.iter().zip(adjustment_paths) {
        pairs.push(read_publication_pair(forecast, adjustment, values)?);
    }
    pairs.sort_by(|left, right| {
        values.compare_scorecard_labels(&left.localDateFrom, &right.localDateFrom)
    });
    // reject overlapping blocks after all pairs have been admitted
    if pairs
        .windows(2)
        .any(|pair| utf16_cmp(&pair[0].localDateTo, &pair[1].localDateFrom).is_ge())
    {
        return Err(effect("publication manifest date intervals overlap"));
    }
    // preserve exact digests for a single package pair
    if pairs.len() == 1 {
        return Ok(pairs.pop().expect("checked one pair"));
    }
    let first = pairs.first().expect("matched pairs are nonempty");
    let last = pairs.last().expect("matched pairs are nonempty");
    Ok(ForecastAdjustmentPerformancePackageInputs {
        adjustmentEvidenceManifestSha256: digest_vector(
            values,
            pairs
                .iter()
                .map(|pair| pair.adjustmentEvidenceManifestSha256.as_str()),
        )?,
        adjustmentEvidenceWatermarkSha256: digest_vector(
            values,
            pairs
                .iter()
                .map(|pair| pair.adjustmentEvidenceWatermarkSha256.as_str()),
        )?,
        forecastTrainingManifestSha256: digest_vector(
            values,
            pairs
                .iter()
                .map(|pair| pair.forecastTrainingManifestSha256.as_str()),
        )?,
        localDateFrom: first.localDateFrom.clone(),
        localDateTo: last.localDateTo.clone(),
        targetCutoffAt: pairs
            .iter()
            .map(|pair| &pair.targetCutoffAt)
            .max_by(|left, right| utf16_cmp(left, right))
            .expect("matched pairs are nonempty")
            .clone(),
    })
}

/// Bind one pair to its exact bytes, cohort, normalized cutoff and edge freeze.
fn read_publication_pair(
    forecast_path: &str,
    adjustment_path: &str,
    values: &mut impl ScorecardCommandValues,
) -> Result<ForecastAdjustmentPerformancePackageInputs, ScorecardCliError> {
    let forecast_bytes = values.read_scorecard_file(forecast_path).map_err(effect)?;
    let adjustment_bytes = values
        .read_scorecard_file(adjustment_path)
        .map_err(effect)?;
    let forecast = values
        .decode_scorecard_json(&forecast_bytes)
        .map_err(ScorecardCliError::Contract)?;
    require_object(&forecast, "scorecard forecast manifest")
        .map_err(ScorecardCliError::Contract)?;
    let adjustment = values
        .decode_scorecard_json(&adjustment_bytes)
        .map_err(ScorecardCliError::Contract)?;
    require_object(&adjustment, "scorecard adjustment manifest")
        .map_err(ScorecardCliError::Contract)?;
    let edge = member(&adjustment, "edgeEvidence");
    require_object(edge, "scorecard adjustment edge evidence")
        .map_err(ScorecardCliError::Contract)?;
    let from = required_text(&forecast, "fromLocalDate", "scorecard from date")?;
    let to = required_text(&forecast, "toLocalDate", "scorecard to date")?;
    // reject cross-cohort manifest pairing before hashing either source
    if !is_wire(member(&adjustment, "fromLocalDate"), &from)
        || !is_wire(member(&adjustment, "toLocalDate"), &to)
    {
        return Err(effect("publication manifests cover different cohorts"));
    }
    let adjustment_hash = digest(values, &adjustment_bytes)?;
    let forecast_hash = digest(values, &forecast_bytes)?;
    let Some(JsonValue::String(cutoff)) = field(&forecast, "createdAtUtc") else {
        return Err(effect("scorecard target cutoff must be a nonempty string"));
    };
    let cutoff = normalize_verified_performance_timestamp_field(
        &"createdAtUtc".encode_utf16().collect::<Vec<_>>(),
        cutoff.clone(),
        values,
    )
    .map_err(effect)?;
    let cutoff = String::from_utf16(&cutoff).map_err(effect)?;
    // match the original nonempty text admission after normalization
    if cutoff.is_empty() {
        return Err(effect("scorecard target cutoff must be a nonempty string"));
    }
    Ok(ForecastAdjustmentPerformancePackageInputs {
        adjustmentEvidenceManifestSha256: adjustment_hash,
        adjustmentEvidenceWatermarkSha256: required_text(
            edge,
            "watermarkSha256",
            "scorecard adjustment watermark",
        )?,
        forecastTrainingManifestSha256: forecast_hash,
        localDateFrom: from,
        localDateTo: to,
        targetCutoffAt: cutoff,
    })
}

/// Use the generated original package input contract for publication bytes.
fn package_inputs_json(inputs: &ForecastAdjustmentPerformancePackageInputs) -> JsonValue {
    object(vec![
        (
            "adjustmentEvidenceManifestSha256",
            wire(&inputs.adjustmentEvidenceManifestSha256),
        ),
        (
            "adjustmentEvidenceWatermarkSha256",
            wire(&inputs.adjustmentEvidenceWatermarkSha256),
        ),
        (
            "forecastTrainingManifestSha256",
            wire(&inputs.forecastTrainingManifestSha256),
        ),
        ("localDateFrom", wire(&inputs.localDateFrom)),
        ("localDateTo", wire(&inputs.localDateTo)),
        ("targetCutoffAt", wire(&inputs.targetCutoffAt)),
    ])
}

/// Hash one chronological identity vector with original canonical framing.
fn digest_vector<'a>(
    values: &mut impl ScorecardCommandValues,
    vector: impl Iterator<Item = &'a str>,
) -> Result<String, ScorecardCliError> {
    let bytes = canonical(values, &JsonValue::Array(vector.map(wire).collect()))?;
    digest(values, &bytes)
}

/// Preserve javascript's default utf-16 code-unit comparison.
fn utf16_cmp(left: &str, right: &str) -> Ordering {
    left.encode_utf16().cmp(right.encode_utf16())
}

/// Read one predecessor nonempty string field with its original diagnostic.
fn required_text(
    value: &JsonValue,
    name: &str,
    description: &str,
) -> Result<String, ScorecardCliError> {
    text(value, name)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| effect(format!("{description} must be a nonempty string")))
}

/// Require one nonempty string without retaining the surrounding wrapper.
fn required_wire_text(value: &JsonValue, description: &str) -> Result<String, ScorecardCliError> {
    let JsonValue::String(units) = value else {
        return Err(effect(format!("{description} must be a nonempty string")));
    };
    String::from_utf16(units)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| effect(format!("{description} must be a nonempty string")))
}

/// Emit the exact original canonical document framing.
fn canonical(
    values: &mut impl ScorecardCommandValues,
    value: &JsonValue,
) -> Result<Vec<u8>, ScorecardCliError> {
    values
        .encode_canonical_json(value)
        .ok_or_else(|| effect("scorecard serialization failed"))
}

/// Hash the supplied bytes without substituting reserialized inputs.
fn digest(
    values: &mut impl ScorecardCommandValues,
    bytes: &[u8],
) -> Result<String, ScorecardCliError> {
    values
        .sha256(bytes)
        .map(|digest| digest.as_str().to_owned())
        .ok_or_else(|| effect("scorecard digest unavailable"))
}

/// Hash the exact original wrapper and release its retained bytes immediately.
fn digest_report(
    values: &mut impl ScorecardCommandValues,
    bytes: Vec<u8>,
) -> Result<String, ScorecardCliError> {
    let hash = digest(values, &bytes);
    drop(bytes);
    hash
}

/// Build one ordered aggregate without a private wire encoder.
fn object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(key, value)| (key.encode_utf16().collect(), value))
            .collect(),
    )
}

/// Preserve one original ordinary effect failure message.
fn effect(error: impl Display) -> ScorecardCliError {
    ScorecardCliError::Effect(error.to_string())
}

#[cfg(test)]
#[path = "forecast_scorecard_cli_test.rs"]
pub(crate) mod tests;
