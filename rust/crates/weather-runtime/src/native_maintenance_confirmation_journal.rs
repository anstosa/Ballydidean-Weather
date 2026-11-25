//! Native append-only blinded confirmation preregistration and family-slot ownership.

use weather_contracts::JsonValue;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};

use crate::{
    NativeMaintenanceCaptureFamily, NativeMaintenanceJournalRecord, NativeMaintenanceJournalStore,
    NativeMaintenanceJournalStoreError, NativeMaintenanceOwnedJournalAddition,
    NativeWirePrimitives,
    maintenance_canonical_json::{exact_keys, field, text, valid_hash},
    native_maintenance_policy_comparison::{object, string},
    native_maintenance_retirement_files_v2::format_digest,
};

const REGISTERED: &str = "confirmation_registered";
const RESULT: &str = "confirmation_result";
const VERSION: &str = "forecast-adjustment-lifecycle-ledger/v2";
const CONFIRMATION_FIELDS: &[&str] = &[
    "candidateKind",
    "candidateSha256",
    "cohortLineageSha256",
    "family",
    "firstTargetAt",
    "gateManifestSha256",
    "intervalEndExclusiveLocalDate",
    "intervalStartLocalDate",
    "reservedKeySha256",
    "sourceLineageSha256",
    "terminalAccessAt",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceConfirmationJournalError {
    Invalid,
    Chronology,
    Genesis,
    SlotOccupied,
    Embargo,
    MemberReused,
    Store(NativeMaintenanceJournalStoreError),
}

impl From<NativeMaintenanceJournalStoreError> for NativeMaintenanceConfirmationJournalError {
    /// Retain the concrete durable journal failure.
    fn from(error: NativeMaintenanceJournalStoreError) -> Self {
        Self::Store(error)
    }
}

/// Exact newly registered or already admitted confirmation identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceConfirmationPreregistration {
    pub registration_sha256: String,
    pub already_registered: bool,
}

/// Read the verified local journal head from the same durable source.
pub fn read_native_maintenance_confirmation_head(
    journal: &mut NativeMaintenanceJournalStore,
) -> Result<String, NativeMaintenanceConfirmationJournalError> {
    journal
        .read_verified(&mut NativeWirePrimitives, &NativeWirePrimitives)?
        .last()
        .map(|record| record.record_sha256.clone())
        .ok_or(NativeMaintenanceConfirmationJournalError::Genesis)
}

/// Bind one original confirmation identity and append it exactly once under the journal lock.
pub fn preregister_native_maintenance_confirmation(
    journal: &mut NativeMaintenanceJournalStore,
    confirmation: &JsonValue,
    input_head_sha256: &str,
    now: &str,
) -> Result<NativeMaintenanceConfirmationPreregistration, NativeMaintenanceConfirmationJournalError>
{
    use NativeMaintenanceConfirmationJournalError as Error;
    let (family, payload, registration_sha256) =
        prepare_registration(confirmation, input_head_sha256, now)?;
    let candidate_sha256 = text(&payload, "candidateSha256").ok_or(Error::Invalid)?;
    let mut already_registered = false;
    journal.mutate_checked(
        &mut NativeWirePrimitives,
        &NativeWirePrimitives,
        |records| {
            // enforce the original lifecycle genesis before any prospective family slot is occupied
            if !records
                .iter()
                .any(|record| kind(record, "lifecycle_genesis"))
            {
                return Err(Error::Genesis);
            }
            let prefix = format!("confirmation/{}/", family.wire());
            let family_records = records
                .iter()
                .filter(|record| {
                    text(&record.payload, "family").as_deref() == Some(family.wire())
                        || record
                            .due_key
                            .as_ref()
                            .is_some_and(|due| due.starts_with(&prefix))
                })
                .collect::<Vec<_>>();
            // only a genuine terminal result or acknowledged/compensated promotion frees a slot
            for registration in family_records
                .iter()
                .copied()
                .filter(|record| kind(record, REGISTERED))
            {
                let identity =
                    text(&registration.payload, "registrationSha256").ok_or(Error::Invalid)?;
                if is_terminal(&family_records, &identity) {
                    continue;
                }
                if identity == registration_sha256 {
                    already_registered = true;
                    return Ok(Vec::new());
                }
                return Err(Error::SlotOccupied);
            }
            // retain the complete source post-disposition embargo
            if let Some(embargo) = family_records
                .iter()
                .rev()
                .find(|record| kind(record, RESULT))
            {
                let next =
                    text(&embargo.payload, "nextConfirmationEligibleAt").ok_or(Error::Invalid)?;
                let next = NativeWirePrimitives
                    .canonical_utc_milliseconds(&next)
                    .ok_or(Error::Invalid)?;
                if NativeWirePrimitives
                    .canonical_utc_milliseconds(now)
                    .ok_or(Error::Invalid)?
                    < next
                {
                    return Err(Error::Embargo);
                }
            }
            let reserved = text(&payload, "reservedKeySha256").ok_or(Error::Invalid)?;
            let source = text(&payload, "sourceLineageSha256").ok_or(Error::Invalid)?;
            let from = text(&payload, "intervalStartLocalDate").ok_or(Error::Invalid)?;
            let to = text(&payload, "intervalEndExclusiveLocalDate").ok_or(Error::Invalid)?;
            // prohibit the previously registered reserved set or an overlapping same-lineage interval
            for prior in records.iter().filter(|record| kind(record, REGISTERED)) {
                let old_reserved =
                    text(&prior.payload, "reservedKeySha256").ok_or(Error::Invalid)?;
                let old_source =
                    text(&prior.payload, "sourceLineageSha256").ok_or(Error::Invalid)?;
                let old_from =
                    text(&prior.payload, "intervalStartLocalDate").ok_or(Error::Invalid)?;
                let old_to =
                    text(&prior.payload, "intervalEndExclusiveLocalDate").ok_or(Error::Invalid)?;
                if reserved == old_reserved
                    || (source == old_source && old_from < to && from < old_to)
                {
                    return Err(Error::MemberReused);
                }
            }
            Ok(vec![NativeMaintenanceOwnedJournalAddition {
                due_key: Some(format!("{prefix}{candidate_sha256}")),
                kind: REGISTERED.to_owned(),
                payload,
                record_key: format!("{prefix}{registration_sha256}/registered"),
                recorded_at: now.to_owned(),
            }])
        },
    )?;
    Ok(NativeMaintenanceConfirmationPreregistration {
        registration_sha256,
        already_registered,
    })
}

/// Derive the exact source canonical registration hash before adding its lifecycle-only fields.
fn prepare_registration(
    confirmation: &JsonValue,
    input_head: &str,
    now: &str,
) -> Result<
    (NativeMaintenanceCaptureFamily, JsonValue, String),
    NativeMaintenanceConfirmationJournalError,
> {
    use NativeMaintenanceConfirmationJournalError as Error;
    let wire = NativeWirePrimitives;
    if !exact_keys(confirmation, CONFIRMATION_FIELDS) || !valid_hash(input_head) {
        return Err(Error::Invalid);
    }
    let family = match text(confirmation, "family").as_deref() {
        Some("temperature") => NativeMaintenanceCaptureFamily::Temperature,
        Some("wind") => NativeMaintenanceCaptureFamily::Wind,
        Some("rain") => NativeMaintenanceCaptureFamily::Rain,
        _ => return Err(Error::Invalid),
    };
    let (candidate_kind, span) = match family {
        NativeMaintenanceCaptureFamily::Temperature => ("temperature-delayed-mos/v1", 366),
        NativeMaintenanceCaptureFamily::Wind => ("wind-robust-hierarchical-median/v1", 366),
        NativeMaintenanceCaptureFamily::Rain => ("rain-hurdle-wind-occurrence-amount/v1", 334),
    };
    if text(confirmation, "candidateKind").as_deref() != Some(candidate_kind) {
        return Err(Error::Invalid);
    }
    for key in [
        "candidateSha256",
        "cohortLineageSha256",
        "gateManifestSha256",
        "reservedKeySha256",
        "sourceLineageSha256",
    ] {
        if !valid_hash(&text(confirmation, key).ok_or(Error::Invalid)?) {
            return Err(Error::Invalid);
        }
    }
    let first = instant(confirmation, "firstTargetAt").ok_or(Error::Invalid)?;
    let terminal = instant(confirmation, "terminalAccessAt").ok_or(Error::Invalid)?;
    let now = wire.canonical_utc_milliseconds(now).ok_or(Error::Invalid)?;
    let from = local_day(confirmation, "intervalStartLocalDate").ok_or(Error::Invalid)?;
    let to = local_day(confirmation, "intervalEndExclusiveLocalDate").ok_or(Error::Invalid)?;
    // preregistration precedes its first target and terminal access follows the reviewed interval
    if now >= first || terminal <= first || to - from != span {
        return Err(Error::Chronology);
    }
    let JsonValue::Object(fields) = confirmation else {
        return Err(Error::Invalid);
    };
    let mut fields = fields.clone();
    fields.extend([
        ("contractVersion".encode_utf16().collect(), string(VERSION)),
        (
            "inputHeadSha256".encode_utf16().collect(),
            string(input_head),
        ),
    ]);
    let identity_value = JsonValue::Object(fields.clone());
    let identity_bytes = wire
        .encode_sorted_json(&identity_value)
        .ok_or(Error::Invalid)?;
    let registration_sha256 = format_digest(wire.sha256(&identity_bytes));
    let lifecycle = object(vec![
        ("registrationSha256", string(&registration_sha256)),
        ("candidateReportSha256", JsonValue::Null),
        ("actionIdentitySha256", JsonValue::Null),
        ("accessState", string("registered")),
        ("actionState", string("none")),
    ]);
    let JsonValue::Object(lifecycle_fields) = lifecycle else {
        return Err(Error::Invalid);
    };
    fields.extend(lifecycle_fields);
    Ok((family, JsonValue::Object(fields), registration_sha256))
}

/// Recognize only the exact reconciled terminal ownership states.
pub(crate) fn is_terminal(records: &[&NativeMaintenanceJournalRecord], registration: &str) -> bool {
    let find = |name| {
        records.iter().copied().find(|record| {
            kind(record, name)
                && text(&record.payload, "registrationSha256").as_deref() == Some(registration)
        })
    };
    let Some(result) = find(RESULT) else {
        return false;
    };
    if text(&result.payload, "disposition").as_deref() != Some("promoted") {
        return matches!(
            field(&result.payload, "actionIdentitySha256"),
            Some(JsonValue::Null)
        ) && text(&result.payload, "actionState").as_deref() == Some("terminal_no_action");
    }
    find("confirmation_action_acknowledged").is_some()
        || find("confirmation_action_compensation_result").is_some()
}

/// Match a retained journal event discriminant without normalizing it.
fn kind(record: &NativeMaintenanceJournalRecord, expected: &str) -> bool {
    matches!(&record.kind, JsonValue::String(units) if units.iter().copied().eq(expected.encode_utf16()))
}

/// Admit exact UTC milliseconds for one required registration clock.
fn instant(value: &JsonValue, key: &str) -> Option<i64> {
    NativeWirePrimitives.canonical_utc_milliseconds(&text(value, key)?)
}

/// Project a local calendar date into the source's UTC-independent civil day number.
fn local_day(value: &JsonValue, key: &str) -> Option<i64> {
    let date = text(value, key)?;
    if date.len() != 10 {
        return None;
    }
    NativeWirePrimitives
        .canonical_utc_milliseconds(&format!("{date}T00:00:00.000Z"))
        .map(|milliseconds| milliseconds.div_euclid(86_400_000))
}

/// Read the active family registration, or its exact candidate during terminal reconciliation.
pub fn read_native_maintenance_confirmation_registration(
    journal: &mut NativeMaintenanceJournalStore,
    family: NativeMaintenanceCaptureFamily,
    candidate_sha256: &str,
) -> Result<Option<JsonValue>, NativeMaintenanceConfirmationJournalError> {
    use NativeMaintenanceConfirmationJournalError as Error;
    if !valid_hash(candidate_sha256) {
        return Err(Error::Invalid);
    }
    let records = journal.read_verified(&mut NativeWirePrimitives, &NativeWirePrimitives)?;
    let prefix = format!("confirmation/{}/", family.wire());
    let family_records = records
        .iter()
        .filter(|record| {
            text(&record.payload, "family").as_deref() == Some(family.wire())
                || record
                    .due_key
                    .as_ref()
                    .is_some_and(|due| due.starts_with(&prefix))
        })
        .collect::<Vec<_>>();
    // source active selection takes precedence over a historical candidate retry
    for registration in family_records
        .iter()
        .copied()
        .filter(|record| kind(record, REGISTERED))
    {
        let identity = text(&registration.payload, "registrationSha256").ok_or(Error::Invalid)?;
        if !is_terminal(&family_records, &identity) {
            return Ok(Some(registration.payload.clone()));
        }
    }
    Ok(family_records
        .into_iter()
        .find(|record| {
            kind(record, REGISTERED)
                && text(&record.payload, "candidateSha256").as_deref() == Some(candidate_sha256)
        })
        .map(|record| record.payload.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NativeMaintenanceJournalAddition;
    use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf};

    struct Fixture(PathBuf);
    impl Drop for Fixture {
        /// Remove only the bounded isolated ordinary fixture directory.
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Compare canonical registration identities and show durable source-idempotent preregistration.
    #[test]
    fn confirmation_journal_preregister_is_exact_and_idempotent() {
        let mut entropy = [0u8; 8];
        getrandom::fill(&mut entropy).unwrap();
        let path = std::env::temp_dir().join(format!(
            "weather-confirmation-journal-{:016x}",
            u64::from_le_bytes(entropy)
        ));
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        let fixture = Fixture(path);
        let (mut journal, _) = NativeMaintenanceJournalStore::initialize_fixture(
            fixture.0.clone(),
            &mut NativeWirePrimitives,
            &NativeWirePrimitives,
        )
        .unwrap();
        let genesis = object(vec![("contractVersion", string(VERSION))]);
        journal
            .append(
                &[NativeMaintenanceJournalAddition {
                    due_key: None,
                    kind: "lifecycle_genesis",
                    payload: &genesis,
                    record_key: "lifecycle/genesis",
                    recorded_at: "2026-10-31T00:00:00.000Z",
                }],
                &mut NativeWirePrimitives,
                &NativeWirePrimitives,
            )
            .unwrap();
        let head = read_native_maintenance_confirmation_head(&mut journal).unwrap();
        let candidate = "a".repeat(64);
        let confirmation = object(vec![
            ("candidateKind", string("temperature-delayed-mos/v1")),
            ("candidateSha256", string(&candidate)),
            ("cohortLineageSha256", string(&"b".repeat(64))),
            ("family", string("temperature")),
            ("firstTargetAt", string("2026-11-01T07:00:00.000Z")),
            ("gateManifestSha256", string(&"c".repeat(64))),
            ("intervalEndExclusiveLocalDate", string("2027-11-02")),
            ("intervalStartLocalDate", string("2026-11-01")),
            ("reservedKeySha256", string(&"d".repeat(64))),
            ("sourceLineageSha256", string(&"e".repeat(64))),
            ("terminalAccessAt", string("2027-11-10T08:00:00.000Z")),
        ]);
        let result = preregister_native_maintenance_confirmation(
            &mut journal,
            &confirmation,
            &head,
            "2026-10-31T01:00:00.000Z",
        )
        .unwrap();
        assert!(!result.already_registered);
        let replay = preregister_native_maintenance_confirmation(
            &mut journal,
            &confirmation,
            &head,
            "2026-10-31T01:00:00.000Z",
        )
        .unwrap();
        assert!(replay.already_registered);
        assert_eq!(result.registration_sha256, replay.registration_sha256);
        let registration = read_native_maintenance_confirmation_registration(
            &mut journal,
            NativeMaintenanceCaptureFamily::Temperature,
            &candidate,
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            text(&registration, "registrationSha256").as_deref(),
            Some(result.registration_sha256.as_str())
        );
        assert_eq!(
            journal
                .read_verified(&mut NativeWirePrimitives, &NativeWirePrimitives)
                .unwrap()
                .len(),
            2
        );
        let repository = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let json = NativeWirePrimitives
            .encode_sorted_json(&confirmation)
            .unwrap();
        let output = std::process::Command::new("node").current_dir(repository)
            .args(["--input-type=module","-e","import {canonicalJsonBytes,adjustmentSha256} from './scripts/research/adjustment_plaintext_archive.mjs';const p={...JSON.parse(process.argv[1]),contractVersion:'forecast-adjustment-lifecycle-ledger/v2',inputHeadSha256:process.argv[2]};process.stdout.write(adjustmentSha256(canonicalJsonBytes(p)))",std::str::from_utf8(&json).unwrap(),&head]).output().unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, result.registration_sha256.as_bytes());
    }
}
