//! Decode the original fit-only document using existing JSON, hash, and calendar owners.

use weather_contracts::{
    ForecastAdjustmentCandidateV2RuntimeFingerprint, ForecastAdjustmentTrainingProvenance,
    ForecastObservationStation, JsonValue,
};
use weather_database::Sha256Digest;
use weather_worker::wind_maintenance_development::AdmittedWindRowCalendar;

use crate::maintenance_canonical_json::{exact_keys, field, text, valid_hash};
use crate::wind_maintenance_archive_manifest::validate_archive_wind_fit_manifest;
use crate::wind_maintenance_candidate::prepare_wind_insufficiency;
use crate::wind_maintenance_cutoff::{
    admit_wind_maintenance_coverage, select_wind_maintenance_phase,
};
use crate::wind_maintenance_fit::{finish_wind_maintenance_phase, valid_wind_due_month};
use crate::wind_maintenance_input_rows::{
    WindFitRowHeader, admission_headers, training_projection,
};
use crate::wind_maintenance_snapshot_manifest::validate_snapshot_wind_fit_manifest;
use crate::{
    AdmittedWindMaintenanceDate, AdmittedWindMaintenanceEpoch, AdmittedWindMaintenanceInstant,
    AdmittedWindMaintenanceManifest, AdmittedWindMaintenanceManifestMember,
    AdmittedWindMaintenanceOpenedMember, AdmittedWindMaintenanceRow,
    ApiMaintenanceShadowEvidenceValues, PreparedWindMaintenanceCandidate,
    ReadyWindMaintenancePreholdout, WindMaintenanceCandidateError, WindMaintenanceCutoffError,
    WindMaintenanceCutoffInput, WindMaintenanceCutoffOutcome, WindMaintenanceFitError,
    WindMaintenanceLocalCalendar, encode_wind_maintenance_fit_publication,
};

const EPOCH_LAST_DAY: u16 = 401;
const TRAINING_LAST_DAY: usize = 364;
const ROW_MAXIMUM: usize = 4_000_000;
const ARCHIVE_OPENED_MAXIMUM: usize = 1_000_000;
const SNAPSHOT_OPENED_MAXIMUM: usize = 65_536;
const ARCHIVE_VERSION: &str = "adjustment-wind-archive-fit-manifest/v2";

/// Site-local date and hierarchy features for one canonical instant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindFitLocalFeatures {
    pub local_date: String,
    pub calendar: AdmittedWindRowCalendar,
}

/// Date, instant and pinned runtime fingerprint supplied by the existing calendar owner.
pub trait WindFitInputCalendar: WindMaintenanceLocalCalendar {
    /// Admit one exact Gregorian date label and return its local-day ordinal.
    fn wind_fit_date_ordinal(&self, date: &str) -> Option<i64>;
    /// Add calendar labels, not elapsed days in UTC.
    fn wind_fit_add_local_days(&self, date: &str, days: u16) -> Option<String>;
    /// Format one canonical millisecond UTC instant.
    fn wind_fit_utc_at(&self, ms: i64) -> Option<String>;
    /// Derive the original Los Angeles date, season, month, and daypart.
    fn wind_fit_local_features(&self, utc: &str, ms: i64) -> Option<WindFitLocalFeatures>;
    /// Return the same calendar/tzdata identity used to derive the features.
    fn wind_fit_runtime_fingerprint(&self) -> ForecastAdjustmentCandidateV2RuntimeFingerprint;
}

pub(crate) struct ValidatedWindFitMember {
    pub(crate) local_date: String,
    pub(crate) sha256: Sha256Digest,
}

pub(crate) struct ValidatedWindFitManifest {
    pub(crate) from_local_date: String,
    pub(crate) to_local_date: String,
    pub(crate) members: Vec<ValidatedWindFitMember>,
    pub(crate) training_provenance: ForecastAdjustmentTrainingProvenance,
}

/// Ordinary refusal at the original command or fit phase.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WindFitInputError {
    Input,
    RowCeiling,
    Manifest,
    ManifestIdentity,
    Calendar,
    OpenedMember,
    Row,
    Cutoff(WindMaintenanceCutoffError),
    Fit(WindMaintenanceFitError),
    Artifact(WindMaintenanceCandidateError),
}

/// Exact canonical output and the callable fitter's original return code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedWindFitOutput {
    pub bytes: Vec<u8>,
    pub fit_exit_code: u8,
}

struct WindFitInput {
    due_month: String,
    manifest: ValidatedWindFitManifest,
    snapshot: Sha256Digest,
    members: Vec<OpenedWindFitMember>,
    rows: Vec<JsonValue>,
}

struct SelectedWindFitRows {
    headers: Vec<WindFitRowHeader>,
    indices: Vec<usize>,
}

struct OpenedWindFitMember {
    sha256: Sha256Digest,
    source: Option<CutoffClock>,
    target: Option<CutoffClock>,
}

pub(super) struct CutoffClock {
    utc: String,
    ms: i64,
}

impl CutoffClock {
    /// Borrow the instant already resolved by the original clock authority.
    fn admitted(&self) -> AdmittedWindMaintenanceInstant<'_> {
        AdmittedWindMaintenanceInstant::from_canonical(&self.utc, self.ms)
    }
}

/// Verify one real fit-only document and prepare the fixed sandbox output without writing files.
pub fn run_wind_maintenance_document(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    calendar: &impl WindFitInputCalendar,
    stations: &[ForecastObservationStation],
    doc: JsonValue,
) -> Result<PreparedWindFitOutput, WindFitInputError> {
    let input = validate_wind_fit_input(values, calendar, doc)?;
    let epoch_days = fit_epoch(values, calendar, &input.manifest)?;
    let dates = epoch_days
        .iter()
        .map(|(label, ordinal, season)| AdmittedWindMaintenanceDate {
            label,
            ordinal: *ordinal,
            season: *season,
        })
        .collect::<Vec<_>>();
    let due_date = format!("{}-01", input.due_month);
    let due = resolve_midnight(values, calendar, &due_date, false, false)?;
    let end = resolve_midnight(
        values,
        calendar,
        dates[usize::from(EPOCH_LAST_DAY)].label,
        true,
        false,
    )?;
    let training_cutoff =
        resolve_midnight(values, calendar, dates[TRAINING_LAST_DAY].label, true, true)?;
    let epoch = AdmittedWindMaintenanceEpoch {
        local_dates: &dates,
        end_exclusive: end.admitted(),
        final_training_cutoff: training_cutoff.admitted(),
    };
    let selected = admit_phase(values, calendar, &input, &due, epoch)?;
    let prepared = match selected {
        None => prepare_wind_insufficiency(
            values,
            &input.snapshot,
            vec!["maintenance_earlier_receipt_provenance".to_owned()],
        )
        .map_err(WindFitInputError::Artifact)?,
        Some(selected) => {
            let (training, identity) =
                training_projection(values, input.rows, selected.headers, &selected.indices)?;
            // release the admitted join material before event or bootstrap allocation
            drop(input.members);
            finish_wind_maintenance_phase(
                values,
                &dates,
                ReadyWindMaintenancePreholdout {
                    due_month: &input.due_month,
                    d0: dates[0].label,
                    d401: dates[usize::from(EPOCH_LAST_DAY)].label,
                    final_training_cutoff: epoch.final_training_cutoff,
                    snapshot_manifest_sha256: &input.snapshot,
                    expected_training_dates: dates[..=TRAINING_LAST_DAY]
                        .iter()
                        .map(|date| (date.label, date.season))
                        .collect(),
                    training_rows: training.iter().map(|row| row.admitted()).collect(),
                },
                identity.as_ref(),
                &calendar.wind_fit_runtime_fingerprint(),
                &input.manifest.training_provenance,
                stations,
            )
            .map_err(WindFitInputError::Fit)?
        }
    };
    let fit_exit_code = match &prepared {
        PreparedWindMaintenanceCandidate::Insufficient { .. } => 2,
        PreparedWindMaintenanceCandidate::DevelopmentCandidate(_) => 0,
    };
    let bytes = encode_wind_maintenance_fit_publication(values, &input.due_month, &prepared)
        .map_err(WindFitInputError::Artifact)?;
    Ok(PreparedWindFitOutput {
        bytes,
        fit_exit_code,
    })
}

/// Join immutable receipts and check every row's clock before selecting any training metrics.
fn admit_phase(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    calendar: &impl WindFitInputCalendar,
    input: &WindFitInput,
    due: &CutoffClock,
    epoch: AdmittedWindMaintenanceEpoch<'_>,
) -> Result<Option<SelectedWindFitRows>, WindFitInputError> {
    let members = input
        .manifest
        .members
        .iter()
        .map(|member| AdmittedWindMaintenanceManifestMember {
            local_date: &member.local_date,
            sha256: &member.sha256,
        })
        .collect::<Vec<_>>();
    let opened = input
        .members
        .iter()
        .map(|member| AdmittedWindMaintenanceOpenedMember {
            member_sha256: &member.sha256,
            // invalid clocks fail on referenced members but cannot be moved before the due cutoff
            maximum_source_receipt_at: member.source.as_ref().unwrap_or(due).admitted(),
            maximum_target_receipt_at: member.target.as_ref().unwrap_or(due).admitted(),
        })
        .collect::<Vec<_>>();
    let view = WindMaintenanceCutoffInput {
        due_month: &input.due_month,
        cutoff: due.admitted(),
        epoch,
        manifest: AdmittedWindMaintenanceManifest {
            identity: &input.snapshot,
            from_local_date: &input.manifest.from_local_date,
            to_local_date: &input.manifest.to_local_date,
            members: &members,
        },
        opened_members: &opened,
        rows: &[],
    };
    // preserve original receipt and manifest failure ordering before reading row clocks
    let covered = admit_wind_maintenance_coverage(&view).map_err(WindFitInputError::Cutoff)?;
    let headers = admission_headers(values, calendar, &input.rows)?;
    let rows = headers
        .iter()
        .enumerate()
        .map(|(index, header)| AdmittedWindMaintenanceRow {
            material: index,
            local_date: &header.features.local_date,
            received_at: header.received_at.as_ref().map(CutoffClock::admitted),
        })
        .collect::<Vec<_>>();
    let result = select_wind_maintenance_phase(
        WindMaintenanceCutoffInput {
            rows: &rows,
            ..view
        },
        &covered,
    );
    // keep only the admitted phase order once the temporary receipt join is released
    let selected = match result {
        WindMaintenanceCutoffOutcome::Refused(_) => None,
        WindMaintenanceCutoffOutcome::Ready(ready) => Some(ready.training_rows),
    };
    Ok(selected.map(|indices| SelectedWindFitRows { headers, indices }))
}

/// Verify the actual manifest's canonical preimage before any epoch or row work.
fn validate_wind_fit_input(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    calendar: &impl WindFitInputCalendar,
    mut doc: JsonValue,
) -> Result<WindFitInput, WindFitInputError> {
    const INPUT_KEYS: [&str; 6] = [
        "contractVersion",
        "manifest",
        "rows",
        "snapshotManifestSha256",
        "dueMonth",
        "openedMembers",
    ];
    // recognize only the original fit-only input document
    if !exact_keys(&doc, &INPUT_KEYS)
        || text(&doc, "contractVersion").as_deref() != Some("wind-maintenance-fit-input/v2")
    {
        return Err(WindFitInputError::Input);
    }
    let manifest = take_field(&mut doc, "manifest").ok_or(WindFitInputError::Input)?;
    let archive = text(&manifest, "contractVersion").as_deref() == Some(ARCHIVE_VERSION);
    let (Some(JsonValue::Array(rows)), Some(JsonValue::Array(opened))) = (
        take_field(&mut doc, "rows"),
        take_field(&mut doc, "openedMembers"),
    ) else {
        return Err(WindFitInputError::RowCeiling);
    };
    let opened_maximum = if archive {
        ARCHIVE_OPENED_MAXIMUM
    } else {
        SNAPSHOT_OPENED_MAXIMUM
    };
    // retain the original two metadata ceilings and common row ceiling before iterating them
    if rows.len() > ROW_MAXIMUM || opened.len() > opened_maximum {
        return Err(WindFitInputError::RowCeiling);
    }
    let due_month = text(&doc, "dueMonth")
        .filter(|month| valid_wind_due_month(month))
        .ok_or(WindFitInputError::Input)?;
    let claimed = text(&doc, "snapshotManifestSha256")
        .filter(|hash| valid_hash(hash))
        .ok_or(WindFitInputError::Input)?;
    let verified = if archive && field(&manifest, "epochWitnessSha256").is_some() {
        validate_archive_wind_fit_manifest(values, calendar, &manifest)
    } else {
        validate_snapshot_wind_fit_manifest(calendar, &manifest)
    }
    .ok_or(WindFitInputError::Manifest)?;
    let encoded = values
        .encode_shadow_sorted_json(&manifest)
        .ok_or(WindFitInputError::ManifestIdentity)?;
    let snapshot = values
        .sha256(&encoded)
        .filter(|sum| sum.as_str() == claimed)
        .ok_or(WindFitInputError::ManifestIdentity)?;
    let members = opened
        .iter()
        .map(|item| {
            Some(OpenedWindFitMember {
                sha256: Sha256Digest::admitted(text(item, "memberSha256")?),
                source: fit_clock(values, text(item, "maximumSourceReceiptAt")),
                target: fit_clock(values, text(item, "maximumTargetReceiptAt")),
            })
        })
        .collect::<Option<Vec<_>>>()
        .ok_or(WindFitInputError::OpenedMember)?;
    Ok(WindFitInput {
        due_month,
        manifest: verified,
        snapshot,
        members,
        rows,
    })
}

/// Move a large decoded field out without deep-copying its rows or members.
fn take_field(value: &mut JsonValue, key: &str) -> Option<JsonValue> {
    let JsonValue::Object(fields) = value else {
        return None;
    };
    let index = fields
        .iter()
        .position(|(name, _)| name.iter().copied().eq(key.encode_utf16()))?;
    Some(fields.swap_remove(index).1)
}

/// Project the exact manifest hashes used by both original schema arms.
pub(crate) fn manifest_training_provenance(
    doc: &JsonValue,
) -> Option<ForecastAdjustmentTrainingProvenance> {
    let required = |field| text(doc, field).filter(|value| valid_hash(value));
    Some(ForecastAdjustmentTrainingProvenance {
        aggregationContractSha256: required("aggregationContractSha256")?,
        coordinateManifestSha256: required("coordinateManifestSha256")?,
        metricEligibilitySha256: required("metricEligibilitySha256")?,
        observationSourceLineageSha256: required("sourceLineageSha256")?,
        observationStationManifestSha256: required("stationManifestSha256")?,
        spatialWeightSha256: required("spatialWeightsSha256")?,
    })
}

type WindEpochDay = (
    String,
    i64,
    weather_contracts::ForecastAdjustmentCoefficientSeasonEnum,
);

/// Derive the original 402 local-day labels and season ownership without UTC stepping.
fn fit_epoch(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    calendar: &impl WindFitInputCalendar,
    manifest: &ValidatedWindFitManifest,
) -> Result<Vec<WindEpochDay>, WindFitInputError> {
    let start = calendar
        .wind_fit_date_ordinal(&manifest.from_local_date)
        .ok_or(WindFitInputError::Calendar)?;
    let end = calendar
        .wind_fit_date_ordinal(&manifest.to_local_date)
        .ok_or(WindFitInputError::Calendar)?;
    // bind the exact immutable epoch before allocating day views
    if start.checked_add(i64::from(EPOCH_LAST_DAY)) != Some(end) {
        return Err(WindFitInputError::Calendar);
    }
    (0..=EPOCH_LAST_DAY)
        .map(|offset| {
            let label = calendar.wind_fit_add_local_days(&manifest.from_local_date, offset)?;
            let ordinal = calendar.wind_fit_date_ordinal(&label)?;
            let noon = format!("{label}T12:00:00.000Z");
            let season = calendar
                .wind_fit_local_features(&noon, values.canonical_utc_epoch_milliseconds(&noon)?)?;
            (ordinal == start.checked_add(i64::from(offset))? && season.local_date == label)
                .then_some((label, ordinal, season.calendar.season))
        })
        .collect::<Option<Vec<_>>>()
        .ok_or(WindFitInputError::Calendar)
}

/// Resolve one site midnight or its last preceding millisecond via the calendar owner.
fn resolve_midnight(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    calendar: &impl WindFitInputCalendar,
    date: &str,
    next_day: bool,
    last_millisecond: bool,
) -> Result<CutoffClock, WindFitInputError> {
    let midnight = calendar
        .wind_local_midnight_milliseconds(date, next_day)
        .ok_or(WindFitInputError::Calendar)?;
    let ms = midnight
        .checked_sub(i64::from(last_millisecond))
        .ok_or(WindFitInputError::Calendar)?;
    let utc = calendar.wind_fit_utc_at(ms);
    fit_clock(values, utc)
        .filter(|clock| clock.ms == ms)
        .ok_or(WindFitInputError::Calendar)
}

/// Retain exactly a millisecond UTC input, never a normalizable alternate receipt.
pub(super) fn fit_clock(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    raw: Option<String>,
) -> Option<CutoffClock> {
    let utc = raw.filter(|clock| clock.len() == 24)?;
    let ms = values.canonical_utc_epoch_milliseconds(&utc)?;
    Some(CutoffClock { utc, ms })
}
