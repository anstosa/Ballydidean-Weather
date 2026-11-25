//! Closed schema and semantic admission for one forecast-training export row.

use std::cmp::Ordering;
use std::fmt::{self, Display};

use weather_contracts::JsonValue;

use crate::forecast_training_package_manifest::ForecastTrainingPackageSourceIdentity;
use crate::maintenance_canonical_json::{exact_keys, field, text, valid_hash};

const MAXIMUM_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;
const ROW_KEYS: &[&str] = &[
    "adapter_contracts",
    "collision_count",
    "content_hashes",
    "contract_epoch",
    "dataset",
    "exclusion_reason_codes",
    "ingestion_run_ids",
    "physical_station_key",
    "provider_family",
    "record_kind",
    "received_at",
    "reference_at",
    "reference_kind",
    "relative_humidity_percent",
    "site_key",
    "source_config_fingerprints",
    "source_keys",
    "target_lead_hours",
    "temperature_c",
    "upstream_model",
    "valid_at",
    "wind_direction_degrees",
    "wind_gust_mps",
    "wind_speed_mps",
];
const STATION_EXCLUSION_REASONS: &[&str] = &[
    "metric_ineligible",
    "metric_missing",
    "quality_flag_rejected",
    "quality_status_rejected",
    "source_interval_out_of_range",
    "source_superseded",
    "station_coverage_insufficient",
    "station_direction_calm",
    "station_gust_coverage_incomplete",
];

/// Complete source catalog in the original canonical source-key order.
pub const FORECAST_TRAINING_SOURCE_IDENTITIES: &[ForecastTrainingPackageSourceIdentity] = &[
    ForecastTrainingPackageSourceIdentity {
        source_key: "ambient-maxweather-observations-v1",
        source_config_fingerprint: "7a7528a6278924ca5280a1a6045b6647b7e660b112d7fa3008c542a17ff99df4",
        adapter_contract: "ambient-device-data/v1",
    },
    ForecastTrainingPackageSourceIdentity {
        source_key: "ambient-merlin-observations-v1",
        source_config_fingerprint: "c3829701bfc25a050022dc3965569d3a87376e8a43b5fdcb7621533f1ae3c65d",
        adapter_contract: "ambient-device-data/v1",
    },
    ForecastTrainingPackageSourceIdentity {
        source_key: "ecowitt-88f15505d89f-local-live-v1",
        source_config_fingerprint: "0a44488714d0fa807b924f8aea14965b437722e8cf9f8eae4bc8c81da8a0149d",
        adapter_contract: "ecowitt-local-live/v1",
    },
    ForecastTrainingPackageSourceIdentity {
        source_key: "netatmo-nearby-observations-v1",
        source_config_fingerprint: "5495917dd2465a32d9878e73c68781a229b432901cdc4867875726351efbdbbc",
        adapter_contract: "netatmo-public-measures/v1",
    },
    ForecastTrainingPackageSourceIdentity {
        source_key: "open-meteo-forecast-v4",
        source_config_fingerprint: "ceb83ac4ba3ddc421a31043794ad450a859ecc31643506f93f64a28feb15e5b4",
        adapter_contract: "forecast-daily/v4",
    },
    ForecastTrainingPackageSourceIdentity {
        source_key: "open-meteo-previous-runs-v1",
        source_config_fingerprint: "3a311d67d08aa3f9dedc2dbb8382d4cf11f945439d50c328a93874fc0a44538e",
        adapter_contract: "previous-runs-hourly/v1",
    },
    ForecastTrainingPackageSourceIdentity {
        source_key: "tempest-126537-observations-v2",
        source_config_fingerprint: "34dafbd6584c93d55ed4d3d43dc7e74a0876165d4ddfc921413f7b826dff7ab7",
        adapter_contract: "tempest-observations/v2",
    },
    ForecastTrainingPackageSourceIdentity {
        source_key: "tempest-168853-observations-v2",
        source_config_fingerprint: "1c7a402337a44a5441775246cbc02da7994599dad6ab83dc04b248303facfea5",
        adapter_contract: "tempest-observations/v2",
    },
    ForecastTrainingPackageSourceIdentity {
        source_key: "tempest-201058-observations-v2",
        source_config_fingerprint: "a61cce798cddf682da9608dc245659fc7734a6d5304068939d3115ae7d81a50e",
        adapter_contract: "tempest-observations/v2",
    },
    ForecastTrainingPackageSourceIdentity {
        source_key: "tempest-203055-observations-v2",
        source_config_fingerprint: "9ead4c5359a6a9640f334be91397180aa62b90b0f0ce813b9ff26fe84537acc4",
        adapter_contract: "tempest-observations/v2",
    },
    ForecastTrainingPackageSourceIdentity {
        source_key: "tempest-225947-observations-v2",
        source_config_fingerprint: "b4dd6105d9a56a7c5d0dc4063f830e1cf28d693222a8de15536dd83d3a6178c4",
        adapter_contract: "tempest-observations/v2",
    },
    ForecastTrainingPackageSourceIdentity {
        source_key: "tempest-38270-observations-v2",
        source_config_fingerprint: "ce162067aced4ab3522fb83145a21e608ff24dec189097726188e96fd6cca52f",
        adapter_contract: "tempest-observations/v2",
    },
    ForecastTrainingPackageSourceIdentity {
        source_key: "tempest-64255-observations-v2",
        source_config_fingerprint: "8eb488a358375fc3526347d9ef6c9f23080095a22ea874a42ec400b0317d868a",
        adapter_contract: "tempest-observations/v2",
    },
    ForecastTrainingPackageSourceIdentity {
        source_key: "wunderground-maxweather-history-v1",
        source_config_fingerprint: "52dda6c5444d0a234fbe23d6218027d417ac966ecf291a7d5dfff42fd0dc207c",
        adapter_contract: "wunderground-pws-history/v1",
    },
];

#[derive(Debug, Clone, Copy)]
struct StationSourceBoundary {
    source_key: &'static str,
    station_key: &'static str,
    from_valid_at: Option<&'static str>,
    to_valid_at: Option<&'static str>,
}

const STATION_SOURCE_BOUNDARIES: &[StationSourceBoundary] = &[
    StationSourceBoundary {
        source_key: "ambient-maxweather-observations-v1",
        station_key: "ambient-maxweather",
        from_valid_at: Some("2026-08-24T00:00:00.000Z"),
        to_valid_at: None,
    },
    StationSourceBoundary {
        source_key: "ambient-merlin-observations-v1",
        station_key: "ambient-merlin",
        from_valid_at: Some("2021-01-01T00:00:00.000Z"),
        to_valid_at: None,
    },
    StationSourceBoundary {
        source_key: "ecowitt-88f15505d89f-local-live-v1",
        station_key: "ballydidean-ecowitt",
        from_valid_at: None,
        to_valid_at: None,
    },
    StationSourceBoundary {
        source_key: "netatmo-nearby-observations-v1",
        station_key: "netatmo-nearby",
        from_valid_at: Some("2022-06-21T00:00:00.000Z"),
        to_valid_at: None,
    },
    StationSourceBoundary {
        source_key: "tempest-126537-observations-v2",
        station_key: "tempest-126537",
        from_valid_at: Some("2023-12-17T00:00:00.000Z"),
        to_valid_at: None,
    },
    StationSourceBoundary {
        source_key: "tempest-168853-observations-v2",
        station_key: "tempest-168853",
        from_valid_at: Some("2025-01-22T00:00:00.000Z"),
        to_valid_at: None,
    },
    StationSourceBoundary {
        source_key: "tempest-201058-observations-v2",
        station_key: "tempest-201058",
        from_valid_at: Some("2025-12-22T00:00:00.000Z"),
        to_valid_at: None,
    },
    StationSourceBoundary {
        source_key: "tempest-203055-observations-v2",
        station_key: "tempest-203055",
        from_valid_at: Some("2025-12-25T00:00:00.000Z"),
        to_valid_at: None,
    },
    StationSourceBoundary {
        source_key: "tempest-225947-observations-v2",
        station_key: "tempest-225947",
        from_valid_at: Some("2026-07-14T00:00:00.000Z"),
        to_valid_at: None,
    },
    StationSourceBoundary {
        source_key: "tempest-38270-observations-v2",
        station_key: "tempest-38270",
        from_valid_at: Some("2021-01-04T00:00:00.000Z"),
        to_valid_at: None,
    },
    StationSourceBoundary {
        source_key: "tempest-64255-observations-v2",
        station_key: "tempest-64255",
        from_valid_at: Some("2021-12-10T00:00:00.000Z"),
        to_valid_at: None,
    },
    StationSourceBoundary {
        source_key: "wunderground-maxweather-history-v1",
        station_key: "ambient-maxweather",
        from_valid_at: Some("2024-11-29T00:00:00.000Z"),
        to_valid_at: Some("2026-08-24T00:00:00.000Z"),
    },
];

/// Approved JavaScript-compatible UTC and Ballydidean calendar owner.
pub trait ForecastTrainingPackageRowTime {
    /// Parse one accepted instant with the original `Date.parse` millisecond behavior.
    fn utc_epoch_milliseconds(&self, value: &str) -> Option<i64>;

    /// Format one epoch millisecond with the original `Date.toISOString` spelling.
    fn canonical_utc(&self, epoch_ms: i64) -> Option<String>;

    /// Resolve the `America/Los_Angeles` calendar label for one instant.
    fn los_angeles_local_date(&self, epoch_ms: i64) -> Option<String>;
}

/// Closed forecast-training member record kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastTrainingRecordKind {
    StationHour,
    FixedLeadAnchor,
    LegacyV4RetrievalSnapshot,
}

impl ForecastTrainingRecordKind {
    /// Return the exact member directory component used by the predecessor.
    pub const fn member_component(self) -> &'static str {
        // preserve the original kebab-case partition grammar
        match self {
            Self::StationHour => "station-hour",
            Self::FixedLeadAnchor => "fixed-lead-anchor",
            Self::LegacyV4RetrievalSnapshot => "legacy-v4-retrieval",
        }
    }
}

/// Five admitted metrics retained for coverage aggregation and downstream projection.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ForecastTrainingRowMetrics {
    pub relative_humidity_percent: Option<f64>,
    pub temperature_c: Option<f64>,
    pub wind_direction_degrees: Option<f64>,
    pub wind_gust_mps: Option<f64>,
    pub wind_speed_mps: Option<f64>,
}

impl ForecastTrainingRowMetrics {
    /// Report whether at least one export metric is populated.
    pub fn any_present(self) -> bool {
        self.relative_humidity_percent.is_some()
            || self.temperature_c.is_some()
            || self.wind_direction_degrees.is_some()
            || self.wind_gust_mps.is_some()
            || self.wind_speed_mps.is_some()
    }
}

/// Typed evidence returned after complete row admission.
#[derive(Debug, Clone, PartialEq)]
pub struct ValidatedForecastTrainingPackageRow {
    pub row: JsonValue,
    pub valid_at: String,
    pub valid_at_epoch_ms: i64,
    pub local_date: String,
    pub record_kind: ForecastTrainingRecordKind,
    pub station_key: Option<String>,
    pub member_path: String,
    pub source_identities: Vec<ForecastTrainingPackageSourceIdentity>,
    pub metrics: ForecastTrainingRowMetrics,
}

/// Closed failures from the native export-row contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForecastTrainingPackageRowError {
    UnexpectedFields,
    WrongSite,
    InvalidLineage,
    SourceCollision,
    InvalidInstant(&'static str),
    InvalidMetric(&'static str),
    DateRange,
    StationIdentity,
    StationExclusion,
    StationProvenance,
    StationGap,
    StationLineage(String),
    ForecastSource,
    ForecastExclusion,
    PreviousRunsIdentity,
    LegacyV4Identity,
    LegacyV4Lead,
    RecordKind,
}

impl Display for ForecastTrainingPackageRowError {
    /// Preserve the predecessor's row diagnostics at the verifier boundary.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // select the exact stable failure text
        match self {
            Self::UnexpectedFields => formatter.write_str("export row has unexpected fields"),
            Self::WrongSite => formatter.write_str("export row has the wrong site identity"),
            Self::InvalidLineage => formatter.write_str("export row lineage arrays are invalid"),
            Self::SourceCollision => formatter.write_str("export row contains a source collision"),
            Self::InvalidInstant(name) => write!(formatter, "{name} must be an ISO UTC instant"),
            Self::InvalidMetric(name) => write!(formatter, "export row {name} is invalid"),
            Self::DateRange => {
                formatter.write_str("export row falls outside requested local dates")
            }
            Self::StationIdentity => formatter.write_str("station-hour identity is invalid"),
            Self::StationExclusion => {
                formatter.write_str("station-hour exclusion reason is invalid")
            }
            Self::StationProvenance => formatter.write_str("station-hour provenance is incomplete"),
            Self::StationGap => formatter.write_str("station-hour gap evidence is invalid"),
            Self::StationLineage(source_key) => {
                write!(formatter, "station lineage mismatch for {source_key}")
            }
            Self::ForecastSource => formatter.write_str("forecast source identity is invalid"),
            Self::ForecastExclusion => {
                formatter.write_str("forecast row exclusion reason is invalid")
            }
            Self::PreviousRunsIdentity => {
                formatter.write_str("Previous Runs row identity is invalid")
            }
            Self::LegacyV4Identity => {
                formatter.write_str("legacy v4 retrieval row identity is invalid")
            }
            Self::LegacyV4Lead => formatter.write_str("legacy v4 retrieval lead is invalid"),
            Self::RecordKind => formatter.write_str("export row record kind is invalid"),
        }
    }
}

impl std::error::Error for ForecastTrainingPackageRowError {}

/// Validate one exact export row and derive its canonical package evidence.
pub fn validate_forecast_training_package_row(
    time: &impl ForecastTrainingPackageRowTime,
    row: JsonValue,
    from_local_date: &str,
    to_local_date: &str,
) -> Result<ValidatedForecastTrainingPackageRow, ForecastTrainingPackageRowError> {
    // reject schema drift before reading any row value
    if !exact_keys(&row, ROW_KEYS) {
        return Err(ForecastTrainingPackageRowError::UnexpectedFields);
    }
    // bind every row to the one retained site
    if text(&row, "site_key").as_deref() != Some("ballydidean") {
        return Err(ForecastTrainingPackageRowError::WrongSite);
    }

    let source_keys = text_array(&row, "source_keys")
        .filter(|values| sorted_unique(values) && values.iter().all(|value| safe_component(value)))
        .ok_or(ForecastTrainingPackageRowError::InvalidLineage)?;
    let fingerprints = text_array(&row, "source_config_fingerprints")
        .filter(|values| values.iter().all(|value| valid_hash(value)))
        .ok_or(ForecastTrainingPackageRowError::InvalidLineage)?;
    let adapter_contracts = text_array(&row, "adapter_contracts")
        .filter(|values| values.iter().all(|value| adapter_contract(value)))
        .ok_or(ForecastTrainingPackageRowError::InvalidLineage)?;
    let ingestion_run_ids = ingestion_ids(&row, "ingestion_run_ids")
        .filter(|values| numerically_sorted_unique(values))
        .ok_or(ForecastTrainingPackageRowError::InvalidLineage)?;
    let content_hashes = text_array(&row, "content_hashes")
        .filter(|values| sorted_unique(values) && values.iter().all(|value| valid_hash(value)))
        .ok_or(ForecastTrainingPackageRowError::InvalidLineage)?;
    let exclusion_reasons = text_array(&row, "exclusion_reason_codes")
        .filter(|values| sorted_unique(values) && values.iter().all(|value| safe_component(value)))
        .ok_or(ForecastTrainingPackageRowError::InvalidLineage)?;
    // preserve the three aligned identity columns exactly
    if source_keys.len() != fingerprints.len() || source_keys.len() != adapter_contracts.len() {
        return Err(ForecastTrainingPackageRowError::InvalidLineage);
    }
    // reject every nonzero or noninteger collision witness
    if number(&row, "collision_count") != Some(0.0) {
        return Err(ForecastTrainingPackageRowError::SourceCollision);
    }

    let valid = required_instant(time, &row, "valid_at", "valid_at")?;
    let reference = nullable_instant(time, &row, "reference_at", "reference_at")?;
    let received = nullable_instant(time, &row, "received_at", "received_at")?;
    let local_date = time
        .los_angeles_local_date(valid.1)
        .filter(|value| canonical_date(value))
        .ok_or(ForecastTrainingPackageRowError::InvalidInstant("valid_at"))?;
    let metrics = metrics(&row)?;
    // retain the original lexical comparison of canonical local dates
    if local_date.as_str() < from_local_date || local_date.as_str() > to_local_date {
        return Err(ForecastTrainingPackageRowError::DateRange);
    }

    let record_kind = text(&row, "record_kind");
    let (record_kind, station_key, source_identities) =
        // preserve the station branch before the closed forecast grammar
        if record_kind.as_deref() == Some("station_hour") {
            validate_station_row(
                &row,
                &valid.0,
                &source_keys,
                &fingerprints,
                &adapter_contracts,
                &ingestion_run_ids,
                &content_hashes,
                &exclusion_reasons,
                received.is_some(),
                metrics,
            )?
        } else {
            validate_forecast_row(
                &row,
                valid.1,
                reference.as_ref().map(|value| value.1),
                received.is_some(),
                &source_keys,
                &fingerprints,
                &adapter_contracts,
                &ingestion_run_ids,
                &content_hashes,
                &exclusion_reasons,
                metrics,
            )?
        };
    let station_component = station_key.as_deref().unwrap_or("forecast");
    let member_path = format!(
        "members/{}/{}/{}.jsonl.gz",
        local_date,
        record_kind.member_component(),
        station_component
    );
    // enforce the exact safe-component grammar at the derived path boundary
    if !safe_component(&local_date)
        || !safe_component(record_kind.member_component())
        || !safe_component(station_component)
    {
        return Err(ForecastTrainingPackageRowError::RecordKind);
    }

    Ok(ValidatedForecastTrainingPackageRow {
        row,
        valid_at: valid.0,
        valid_at_epoch_ms: valid.1,
        local_date,
        record_kind,
        station_key,
        member_path,
        source_identities,
        metrics,
    })
}

/// Validate one station-hour identity including explicit empty grid rows.
#[allow(clippy::too_many_arguments)]
fn validate_station_row(
    row: &JsonValue,
    valid_at: &str,
    source_keys: &[String],
    fingerprints: &[String],
    adapter_contracts: &[String],
    ingestion_run_ids: &[String],
    content_hashes: &[String],
    exclusion_reasons: &[String],
    received_present: bool,
    metrics: ForecastTrainingRowMetrics,
) -> Result<
    (
        ForecastTrainingRecordKind,
        Option<String>,
        Vec<ForecastTrainingPackageSourceIdentity>,
    ),
    ForecastTrainingPackageRowError,
> {
    let station_key = text(row, "physical_station_key")
        .filter(|value| safe_component(value))
        .ok_or(ForecastTrainingPackageRowError::StationIdentity)?;
    let provider = station_provider(&station_key);
    // bind the exact station projection grammar
    if provider.is_none()
        || text(row, "provider_family").as_deref() != provider
        || !is_null(row, "reference_at")
        || !is_null(row, "reference_kind")
        || !is_null(row, "target_lead_hours")
        || !is_null(row, "dataset")
        || !is_null(row, "upstream_model")
        || text(row, "contract_epoch").as_deref() != Some("physical-station-hourly/v1")
    {
        return Err(ForecastTrainingPackageRowError::StationIdentity);
    }
    // reject every station diagnostic outside the frozen vocabulary
    if exclusion_reasons
        .iter()
        .any(|reason| !STATION_EXCLUSION_REASONS.contains(&reason.as_str()))
    {
        return Err(ForecastTrainingPackageRowError::StationExclusion);
    }

    let complete_provenance = !source_keys.is_empty()
        && !ingestion_run_ids.is_empty()
        && !content_hashes.is_empty()
        && received_present;
    let empty_provenance = source_keys.is_empty()
        && ingestion_run_ids.is_empty()
        && content_hashes.is_empty()
        && !received_present;
    // reject partially populated provenance groups
    if !complete_provenance && !empty_provenance {
        return Err(ForecastTrainingPackageRowError::StationProvenance);
    }
    // populated grid cells always require retained source evidence
    if metrics.any_present() && !complete_provenance {
        return Err(ForecastTrainingPackageRowError::StationProvenance);
    }
    let provenance_free_gap = !metrics.any_present() && empty_provenance;
    let coverage_insufficient = exclusion_reasons
        .iter()
        .any(|reason| reason == "station_coverage_insufficient");
    // bind the explicit missing-hour diagnostic in both directions
    if provenance_free_gap != coverage_insufficient {
        return Err(ForecastTrainingPackageRowError::StationGap);
    }

    let mut identities = Vec::with_capacity(source_keys.len());
    // validate every aligned station lineage tuple and source interval
    for (index, source_key) in source_keys.iter().enumerate() {
        let boundary = station_source(source_key);
        let identity = source_identity(source_key);
        // reject any owner, fingerprint, adapter, or accepted-hour drift
        if boundary.is_none()
            || identity.is_none()
            || boundary.is_some_and(|value| value.station_key != station_key)
            || identity.is_some_and(|value| {
                value.source_config_fingerprint != fingerprints[index]
                    || value.adapter_contract != adapter_contracts[index]
            })
            || boundary.is_some_and(|value| {
                value.from_valid_at.is_some_and(|from| valid_at < from)
                    || value.to_valid_at.is_some_and(|to| valid_at > to)
            })
        {
            return Err(ForecastTrainingPackageRowError::StationLineage(
                source_key.clone(),
            ));
        }
        identities.push(identity.expect("checked station identity"));
    }

    Ok((
        ForecastTrainingRecordKind::StationHour,
        Some(station_key),
        identities,
    ))
}

/// Validate one of the two immutable forecast cohort grammars.
#[allow(clippy::too_many_arguments)]
fn validate_forecast_row(
    row: &JsonValue,
    valid_at_epoch_ms: i64,
    reference_at_epoch_ms: Option<i64>,
    received_present: bool,
    source_keys: &[String],
    fingerprints: &[String],
    adapter_contracts: &[String],
    ingestion_run_ids: &[String],
    content_hashes: &[String],
    exclusion_reasons: &[String],
    metrics: ForecastTrainingRowMetrics,
) -> Result<
    (
        ForecastTrainingRecordKind,
        Option<String>,
        Vec<ForecastTrainingPackageSourceIdentity>,
    ),
    ForecastTrainingPackageRowError,
> {
    let identity = source_keys.first().and_then(|key| source_identity(key));
    // require exactly one complete identity from the frozen forecast catalog
    if !is_null(row, "physical_station_key")
        || !is_null(row, "provider_family")
        || source_keys.len() != 1
        || fingerprints.len() != 1
        || adapter_contracts.len() != 1
        || !matches!(
            source_keys[0].as_str(),
            "open-meteo-forecast-v4" | "open-meteo-previous-runs-v1"
        )
        || identity.is_none()
        || identity.is_some_and(|value| {
            value.source_config_fingerprint != fingerprints[0]
                || value.adapter_contract != adapter_contracts[0]
        })
    {
        return Err(ForecastTrainingPackageRowError::ForecastSource);
    }
    // complete forecast cohorts never carry station exclusions
    if !exclusion_reasons.is_empty() {
        return Err(ForecastTrainingPackageRowError::ForecastExclusion);
    }

    let target_lead = number(row, "target_lead_hours");
    let kind = text(row, "record_kind");
    // distinguish the two retained forecast cohort identities
    let record_kind = match kind.as_deref() {
        Some("fixed_lead_anchor") => {
            let accepted_lead = target_lead.is_some_and(|lead| {
                matches!(lead, 24.0 | 48.0 | 72.0 | 96.0 | 120.0 | 144.0 | 168.0)
            });
            // bind the exact Previous Runs fixed-lead row
            if source_keys[0] != "open-meteo-previous-runs-v1"
                || text(row, "reference_kind").as_deref() != Some("fixed_lead_anchor")
                || !is_null(row, "reference_at")
                || text(row, "dataset").as_deref() != Some("previous_runs")
                || text(row, "upstream_model").as_deref() != Some("best_match")
                || text(row, "contract_epoch").as_deref()
                    != Some("open-meteo-previous-runs-best-match/2026-09")
                || !received_present
                || ingestion_run_ids.is_empty()
                || ingestion_run_ids.len() > 2
                || content_hashes.len() != 1
                || !metrics.any_present()
                || !accepted_lead
            {
                return Err(ForecastTrainingPackageRowError::PreviousRunsIdentity);
            }
            ForecastTrainingRecordKind::FixedLeadAnchor
        }
        Some("legacy_v4_retrieval_snapshot") => {
            let lead = target_lead.filter(|value| safe_positive_integer(*value) && *value <= 168.0);
            // bind the unchanged v4 retrieval cohort
            if source_keys[0] != "open-meteo-forecast-v4"
                || text(row, "reference_kind").as_deref() != Some("retrieval_snapshot")
                || reference_at_epoch_ms.is_none()
                || !received_present
                || text(row, "dataset").as_deref() != Some("forecast")
                || text(row, "upstream_model").as_deref() != Some("best_match")
                || text(row, "contract_epoch").as_deref()
                    != Some(
                        "legacy-v4/9d26d9c46dcaacc422c28e854327b11cd710625e092110786010f0687a100d83",
                    )
                || ingestion_run_ids.len() != 1
                || content_hashes.len() != 1
                || !metrics.any_present()
                || lead.is_none()
            {
                return Err(ForecastTrainingPackageRowError::LegacyV4Identity);
            }
            let reference = reference_at_epoch_ms.expect("checked retrieval reference");
            let elapsed = valid_at_epoch_ms
                .checked_sub(reference)
                .ok_or(ForecastTrainingPackageRowError::LegacyV4Lead)?;
            let projected_lead =
                elapsed.div_euclid(3_600_000) + i64::from(elapsed.rem_euclid(3_600_000) != 0);
            // preserve nonnegative elapsed time and the exact ceiling projection
            if elapsed < 0 || Some(projected_lead as f64) != lead {
                return Err(ForecastTrainingPackageRowError::LegacyV4Lead);
            }
            ForecastTrainingRecordKind::LegacyV4RetrievalSnapshot
        }
        _ => return Err(ForecastTrainingPackageRowError::RecordKind),
    };

    Ok((
        record_kind,
        None,
        vec![identity.expect("checked forecast identity")],
    ))
}

/// Read and range-check all five nullable metric fields.
fn metrics(row: &JsonValue) -> Result<ForecastTrainingRowMetrics, ForecastTrainingPackageRowError> {
    Ok(ForecastTrainingRowMetrics {
        relative_humidity_percent: metric(row, "relative_humidity_percent", 0.0, 100.0, false)?,
        temperature_c: metric(row, "temperature_c", -100.0, 70.0, false)?,
        wind_direction_degrees: metric(row, "wind_direction_degrees", 0.0, 360.0, true)?,
        wind_gust_mps: metric(row, "wind_gust_mps", 0.0, 150.0, false)?,
        wind_speed_mps: metric(row, "wind_speed_mps", 0.0, 150.0, false)?,
    })
}

/// Admit one missing or finite bounded metric.
fn metric(
    row: &JsonValue,
    key: &'static str,
    minimum: f64,
    maximum: f64,
    exclusive_maximum: bool,
) -> Result<Option<f64>, ForecastTrainingPackageRowError> {
    let value = field(row, key).ok_or(ForecastTrainingPackageRowError::UnexpectedFields)?;
    // preserve missing values without numeric coercion
    if matches!(value, JsonValue::Null) {
        return Ok(None);
    }
    let JsonValue::Number(value) = value else {
        return Err(ForecastTrainingPackageRowError::InvalidMetric(key));
    };
    // reject nonfinite and physically impossible values
    if !value.is_finite()
        || *value < minimum
        || *value > maximum
        || (exclusive_maximum && *value == maximum)
    {
        return Err(ForecastTrainingPackageRowError::InvalidMetric(key));
    }
    Ok(Some(*value))
}

/// Parse one required instant and return canonical milliseconds plus epoch value.
fn required_instant(
    time: &impl ForecastTrainingPackageRowTime,
    row: &JsonValue,
    key: &str,
    description: &'static str,
) -> Result<(String, i64), ForecastTrainingPackageRowError> {
    let value =
        text(row, key).ok_or(ForecastTrainingPackageRowError::InvalidInstant(description))?;
    admit_instant(time, &value, description)
}

/// Parse one nullable instant without weakening its present-value grammar.
fn nullable_instant(
    time: &impl ForecastTrainingPackageRowTime,
    row: &JsonValue,
    key: &str,
    description: &'static str,
) -> Result<Option<(String, i64)>, ForecastTrainingPackageRowError> {
    let value = field(row, key).ok_or(ForecastTrainingPackageRowError::UnexpectedFields)?;
    // preserve the explicitly nullable timestamp fields
    if matches!(value, JsonValue::Null) {
        return Ok(None);
    }
    let JsonValue::String(value) = value else {
        return Err(ForecastTrainingPackageRowError::InvalidInstant(description));
    };
    let value = String::from_utf16(value)
        .map_err(|_| ForecastTrainingPackageRowError::InvalidInstant(description))?;
    admit_instant(time, &value, description).map(Some)
}

/// Apply the predecessor's ISO grammar and calendar-rollover round trip.
fn admit_instant(
    time: &impl ForecastTrainingPackageRowTime,
    value: &str,
    description: &'static str,
) -> Result<(String, i64), ForecastTrainingPackageRowError> {
    // reject every ambiguous or extended timestamp spelling
    if !iso_utc_instant(value) {
        return Err(ForecastTrainingPackageRowError::InvalidInstant(description));
    }
    let epoch_ms = time
        .utc_epoch_milliseconds(value)
        .ok_or(ForecastTrainingPackageRowError::InvalidInstant(description))?;
    let canonical = time
        .canonical_utc(epoch_ms)
        .filter(|value| canonical_millisecond_instant(value))
        .ok_or(ForecastTrainingPackageRowError::InvalidInstant(description))?;
    // reject Date-compatible calendar rollover while allowing fractional truncation
    if canonical.get(..19) != value.get(..19) {
        return Err(ForecastTrainingPackageRowError::InvalidInstant(description));
    }
    Ok((canonical, epoch_ms))
}

/// Recognize the original bounded UTC wire grammar without a regex dependency.
fn iso_utc_instant(value: &str) -> bool {
    let bytes = value.as_bytes();
    // require the fixed date/time punctuation and terminal UTC marker
    if bytes.len() < 20
        || bytes.get(4) != Some(&b'-')
        || bytes.get(7) != Some(&b'-')
        || bytes.get(10) != Some(&b'T')
        || bytes.get(13) != Some(&b':')
        || bytes.get(16) != Some(&b':')
        || bytes.last() != Some(&b'Z')
    {
        return false;
    }
    let digit_positions = [0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18];
    // require every fixed-width calendar and clock digit
    if digit_positions
        .iter()
        .any(|index| !bytes[*index].is_ascii_digit())
    {
        return false;
    }
    // accept either whole seconds or one through six fractional digits
    if bytes.len() == 20 {
        return true;
    }
    bytes.len() >= 22
        && bytes.len() <= 27
        && bytes.get(19) == Some(&b'.')
        && bytes[20..bytes.len() - 1].iter().all(u8::is_ascii_digit)
}

/// Recognize the exact `Date.toISOString` millisecond result shape.
fn canonical_millisecond_instant(value: &str) -> bool {
    value.len() == 24
        && iso_utc_instant(value)
        && value.as_bytes().get(19) == Some(&b'.')
        && value.as_bytes()[20..23].iter().all(u8::is_ascii_digit)
}

/// Recognize the formatter's canonical local-date label.
fn canonical_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 10
        && bytes.get(4) == Some(&b'-')
        && bytes.get(7) == Some(&b'-')
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
}

/// Decode one exact JSON string array.
fn text_array(row: &JsonValue, key: &str) -> Option<Vec<String>> {
    let JsonValue::Array(values) = field(row, key)? else {
        return None;
    };
    values
        .iter()
        .map(|value| {
            let JsonValue::String(value) = value else {
                return None;
            };
            String::from_utf16(value).ok()
        })
        .collect()
}

/// Decode positive run IDs to their exact base-ten integer spelling.
fn ingestion_ids(row: &JsonValue, key: &str) -> Option<Vec<String>> {
    let JsonValue::Array(values) = field(row, key)? else {
        return None;
    };
    values
        .iter()
        .map(|value| {
            // preserve both accepted JSON number and decimal-string forms
            match value {
                JsonValue::Number(value) if safe_positive_integer(*value) => {
                    Some(format!("{value:.0}"))
                }
                JsonValue::String(value) => String::from_utf16(value)
                    .ok()
                    .filter(|value| positive_decimal(value)),
                _ => None,
            }
        })
        .collect()
}

/// Require one exact positive JavaScript-safe integer.
fn safe_positive_integer(value: f64) -> bool {
    value.is_finite() && value > 0.0 && value <= MAXIMUM_SAFE_INTEGER && value.fract() == 0.0
}

/// Recognize the original positive decimal-string grammar.
fn positive_decimal(value: &str) -> bool {
    !value.is_empty() && !value.starts_with('0') && value.bytes().all(|byte| byte.is_ascii_digit())
}

/// Require lexicographic ascending order without duplicates.
fn sorted_unique(values: &[String]) -> bool {
    values
        .windows(2)
        .all(|pair| pair[0].as_str() < pair[1].as_str())
}

/// Require ascending arbitrary-precision numeric order without equivalent duplicates.
fn numerically_sorted_unique(values: &[String]) -> bool {
    values
        .windows(2)
        .all(|pair| compare_positive_decimals(&pair[0], &pair[1]) == Ordering::Less)
}

/// Compare two already-admitted positive base-ten integers without allocation.
fn compare_positive_decimals(left: &str, right: &str) -> Ordering {
    left.len()
        .cmp(&right.len())
        .then_with(|| left.as_bytes().cmp(right.as_bytes()))
}

/// Recognize a safe package path component.
fn safe_component(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 128
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes.iter().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        })
}

/// Recognize a frozen adapter-contract spelling.
fn adapter_contract(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 128
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes.iter().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'.' | b'_' | b'/' | b'-')
        })
}

/// Read one JSON number without coercion.
fn number(row: &JsonValue, key: &str) -> Option<f64> {
    let JsonValue::Number(value) = field(row, key)? else {
        return None;
    };
    Some(*value)
}

/// Test one required member for JSON null.
fn is_null(row: &JsonValue, key: &str) -> bool {
    matches!(field(row, key), Some(JsonValue::Null))
}

/// Resolve one frozen source identity by exact source key.
fn source_identity(source_key: &str) -> Option<ForecastTrainingPackageSourceIdentity> {
    FORECAST_TRAINING_SOURCE_IDENTITIES
        .iter()
        .copied()
        .find(|identity| identity.source_key == source_key)
}

/// Resolve one station-source owner and accepted instant interval.
fn station_source(source_key: &str) -> Option<StationSourceBoundary> {
    STATION_SOURCE_BOUNDARIES
        .iter()
        .copied()
        .find(|boundary| boundary.source_key == source_key)
}

/// Resolve the fixed provider family for one station key.
fn station_provider(station_key: &str) -> Option<&'static str> {
    // retain the exact predecessor station-to-provider mapping
    match station_key {
        "ambient-maxweather" | "ambient-merlin" => Some("ambient"),
        "ballydidean-ecowitt" => Some("ecowitt"),
        "netatmo-nearby" => Some("netatmo"),
        "tempest-126537" | "tempest-168853" | "tempest-201058" | "tempest-203055"
        | "tempest-225947" | "tempest-38270" | "tempest-64255" => Some("tempest"),
        _ => None,
    }
}

#[cfg(test)]
#[path = "forecast_training_package_row_fixture_test.rs"]
mod tests;
