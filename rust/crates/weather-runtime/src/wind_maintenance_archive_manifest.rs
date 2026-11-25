use weather_contracts::JsonValue;
use weather_database::Sha256Digest;

use crate::ApiMaintenanceShadowEvidenceValues;
use crate::maintenance_canonical_json::{exact_keys, field, text, valid_hash};
use crate::wind_maintenance_input::{
    ValidatedWindFitManifest, ValidatedWindFitMember, WindFitInputCalendar,
    manifest_training_provenance,
};

const ARCHIVE_MANIFEST_VERSION: &str = "adjustment-wind-archive-fit-manifest/v2";
const MAXIMUM_MEMBERS: usize = 1_000_000;
const MAXIMUM_ROWS: u64 = 4_000_000;
const MAXIMUM_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;
const MANIFEST_KEYS: [&str; 13] = [
    "aggregationContractSha256",
    "contractVersion",
    "coordinateManifestSha256",
    "epochWitnessSha256",
    "fromLocalDate",
    "historyRootSha256",
    "members",
    "metricEligibilitySha256",
    "sourceLineageSha256",
    "spatialWeightsSha256",
    "stationManifestSha256",
    "toLocalDate",
    "totalRowCount",
];
const MEMBER_KEYS: [&str; 10] = [
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

/// Validate one closed archive-derived wind population without opening member files.
pub(crate) fn validate_archive_wind_fit_manifest(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    calendar: &impl WindFitInputCalendar,
    doc: &JsonValue,
) -> Option<ValidatedWindFitManifest> {
    // require the exact v2 top-level shape and literal version
    if !exact_keys(doc, &MANIFEST_KEYS) || text(doc, "contractVersion")? != ARCHIVE_MANIFEST_VERSION
    {
        return None;
    }

    let training_provenance = manifest_training_provenance(doc)?;
    let epoch_witness_sha256 = text(doc, "epochWitnessSha256")?;
    let history_root_sha256 = text(doc, "historyRootSha256")?;

    // retain the two archive-only roots only when they use exact digest spelling
    if !valid_hash(&epoch_witness_sha256) || !valid_hash(&history_root_sha256) {
        return None;
    }

    let from_local_date = text(doc, "fromLocalDate")?;
    let to_local_date = text(doc, "toLocalDate")?;
    let from_ordinal = calendar.wind_fit_date_ordinal(&from_local_date)?;
    let to_ordinal = calendar.wind_fit_date_ordinal(&to_local_date)?;

    // require a non-reversed admitted local-date range
    if from_ordinal > to_ordinal {
        return None;
    }

    let JsonValue::Array(member_values) = field(doc, "members")? else {
        return None;
    };
    let total_row_count = unsigned(doc, "totalRowCount")?;

    // bound the complete population before allocating its typed projection
    if member_values.is_empty()
        || member_values.len() > MAXIMUM_MEMBERS
        || total_row_count == 0
        || total_row_count > MAXIMUM_ROWS
    {
        return None;
    }

    let mut members = Vec::with_capacity(member_values.len());
    let mut previous_path = None::<String>;
    let mut derived_row_count = 0u64;

    // validate every closed member in its declared path order
    for member in member_values {
        let (projected, path, row_count) = validate_member(values, calendar, member)?;

        // reject duplicate or reversed archive member paths
        if previous_path
            .as_ref()
            .is_some_and(|previous| previous >= &path)
        {
            return None;
        }

        derived_row_count = derived_row_count.checked_add(row_count)?;
        previous_path = Some(path);
        members.push(projected);
    }

    // bind the declared row total to the exact member population
    if derived_row_count != total_row_count {
        return None;
    }

    Some(ValidatedWindFitManifest {
        from_local_date,
        to_local_date,
        members,
        training_provenance,
    })
}

/// Validate and project one archive member without reading its retained payload.
fn validate_member(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    calendar: &impl WindFitInputCalendar,
    member: &JsonValue,
) -> Option<(ValidatedWindFitMember, String, u64)> {
    // require the exact shared member metadata shape
    if !exact_keys(member, &MEMBER_KEYS) {
        return None;
    }

    let local_date = text(member, "localDate")?;
    calendar.wind_fit_date_ordinal(&local_date)?;
    let sha256 = text(member, "sha256")?;
    let path = text(member, "path")?;
    let record_kind = text(member, "recordKind")?;
    let row_count = unsigned(member, "rowCount")?;
    let size_bytes = unsigned(member, "sizeBytes")?;
    let plaintext_bytes = unsigned(member, "plaintextBytes")?;
    let minimum_valid_at = text(member, "minValidAt")?;
    let maximum_valid_at = text(member, "maxValidAt")?;

    // require the literal member sizes, identity and one-row population
    if !valid_hash(&sha256)
        || row_count != 1
        || size_bytes == 0
        || plaintext_bytes == 0
        || minimum_valid_at != maximum_valid_at
        || !wind_maintenance_instant(values, &minimum_valid_at)
    {
        return None;
    }

    let target = record_kind == "target_revision";

    // require one recognized record kind and its original station nullability
    if (!target && record_kind != "actual_best_match")
        || target != !matches!(field(member, "stationKey")?, JsonValue::Null)
        || !archive_member_path(&path, &local_date)
    {
        return None;
    }

    Some((
        ValidatedWindFitMember {
            local_date,
            sha256: Sha256Digest::admitted(sha256),
        },
        path,
        row_count,
    ))
}

/// Decode one safe unsigned JavaScript number.
fn unsigned(doc: &JsonValue, key: &str) -> Option<u64> {
    let JsonValue::Number(value) = field(doc, key)? else {
        return None;
    };

    // preserve Number.isSafeInteger and nonnegative count semantics
    if !value.is_finite() || value.fract() != 0.0 || *value < 0.0 || *value > MAXIMUM_SAFE_INTEGER {
        return None;
    }

    Some(*value as u64)
}

/// Recognize the fixed archive member path grammar without normalizing it.
fn archive_member_path(path: &str, local_date: &str) -> bool {
    let Some(suffix) = path.strip_prefix("archive-members/") else {
        return false;
    };
    let mut parts = suffix.split('/');
    let date = parts.next();
    let family = parts.next();
    let name = parts.next();

    // require exactly the date, family and hash path segments
    if parts.next().is_some() || date != Some(local_date) {
        return false;
    }

    matches!(family, Some("forecast" | "target")) && name.is_some_and(valid_hash)
}

/// Recognize the exact millisecond UTC representation used by the journal.
fn wind_maintenance_instant(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    instant: &str,
) -> bool {
    let bytes = instant.as_bytes();

    // require YYYY-MM-DDTHH:mm:ss.sssZ before asking the approved clock owner
    if bytes.len() != 24
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
        || bytes[19] != b'.'
        || bytes[23] != b'Z'
        || bytes.iter().enumerate().any(|(index, byte)| {
            !matches!(index, 4 | 7 | 10 | 13 | 16 | 19 | 23) && !byte.is_ascii_digit()
        })
    {
        return false;
    }

    values.canonical_utc_epoch_milliseconds(instant).is_some()
}
