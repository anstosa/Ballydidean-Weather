//! Source terminal lifecycle events, including atomic staged C/T/F promotion authority.

use weather_contracts::JsonValue;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};

use crate::{
    NativeFamilyReleaseCurrent, NativeMaintenanceArchiveJournalError,
    NativeMaintenanceCaptureFamily, NativeMaintenanceJournalRecord, NativeMaintenanceJournalStore,
    NativeMaintenanceJournalStoreError, NativeMaintenanceOwnedJournalAddition,
    NativeMaintenanceReleaseLease, NativeWirePrimitives,
    maintenance_canonical_json::{field, text, valid_hash},
    native_maintenance_archive_journal::{addition, object, string, trusted_clock},
    native_maintenance_retirement_files_v2::format_digest,
};

const DAY: i64 = 86_400_000;
const REGISTERED: &str = "confirmation_registered";
const COMPLETE: &str = "confirmation_member_complete";
const RESULT: &str = "confirmation_result";
const PREPARED: &str = "confirmation_action_prepared";
const QUALIFIED_AUTHORITY: &str = "qualified_terminal_authority";
const NO_ACTION_AUTHORITY: &str = "no_action_terminal_authority";

/// The source lifecycle's closed evaluated member dispositions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceTerminalResultDisposition {
    Promoted,
    Rejected,
    ResourceRefused,
    SupportFailed,
}

impl NativeMaintenanceTerminalResultDisposition {
    /// Retain the exact original lifecycle value.
    pub const fn wire(self) -> &'static str {
        match self {
            Self::Promoted => "promoted",
            Self::Rejected => "rejected",
            Self::ResourceRefused => "resource_refused",
            Self::SupportFailed => "support_failed",
        }
    }
}

/// Exact source terminal result request.
#[derive(Clone, Copy, Debug)]
pub struct NativeMaintenanceTerminalResultInput<'a> {
    pub family: NativeMaintenanceCaptureFamily,
    pub registration_sha256: &'a str,
    pub candidate_report_sha256: &'a str,
    pub action_identity_sha256: Option<&'a str>,
    pub disposition: NativeMaintenanceTerminalResultDisposition,
    pub next_confirmation_eligible_at: &'a str,
    pub now: &'a str,
}

/// Exact common C/T/F staged authority documents before any privileged owner transition.
pub struct NativeMaintenanceTerminalAuthorityInput<'a> {
    pub action: &'a JsonValue,
    pub anchor: &'a JsonValue,
    pub current: &'a NativeFamilyReleaseCurrent,
    pub family: NativeMaintenanceCaptureFamily,
    pub finalization_proof: &'a JsonValue,
    pub now: &'a str,
    pub registration_sha256: &'a str,
    pub seal: &'a JsonValue,
}

/// Promotion staging additionally binds the original live release fence.
pub struct NativeMaintenanceQualifiedAuthorityInput<'a> {
    pub authority: NativeMaintenanceTerminalAuthorityInput<'a>,
    pub lease: &'a NativeMaintenanceReleaseLease,
    pub candidate_report_sha256: &'a str,
    pub next_confirmation_eligible_at: &'a str,
}

/// Exact retained qualification authority and the atomically committed result.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceStagedQualifiedAuthority {
    pub authority: JsonValue,
    pub local_result: JsonValue,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceTerminalResultJournalError {
    Input,
    Clock,
    Member,
    Identity,
    Collision,
    Fence,
    Archive(NativeMaintenanceArchiveJournalError),
    Store(NativeMaintenanceJournalStoreError),
}

use NativeMaintenanceTerminalResultJournalError as Error;

impl From<NativeMaintenanceJournalStoreError> for Error {
    fn from(value: NativeMaintenanceJournalStoreError) -> Self {
        Self::Store(value)
    }
}
impl From<NativeMaintenanceArchiveJournalError> for Error {
    fn from(value: NativeMaintenanceArchiveJournalError) -> Self {
        Self::Archive(value)
    }
}

/// Record one exact completed member result without rescoring or reassigning its action.
pub fn record_native_maintenance_confirmation_result(
    journal: &mut NativeMaintenanceJournalStore,
    input: NativeMaintenanceTerminalResultInput<'_>,
) -> Result<JsonValue, Error> {
    admit_result(input, input.now)?;
    let mut result = None;
    journal.mutate_checked::<Error>(
        &mut NativeWirePrimitives,
        &NativeWirePrimitives,
        |records| {
            let (registration, complete) =
                complete_member(records, input.family, input.registration_sha256)?;
            require_unique_action(
                records,
                input.registration_sha256,
                input.action_identity_sha256,
            )?;
            let payload = result_payload(registration, complete, input)?;
            let event = result_event(complete, input, payload.clone())?;
            let mut additions = Vec::new();
            append_exact(records, &mut additions, event)?;
            result = Some(payload);
            Ok(additions)
        },
    )?;
    result.ok_or(Error::Member)
}

/// Atomically persist exact qualified result, release preparation and authority for crash recovery.
pub fn stage_native_maintenance_qualified_terminal_authority(
    journal: &mut NativeMaintenanceJournalStore,
    input: NativeMaintenanceQualifiedAuthorityInput<'_>,
) -> Result<NativeMaintenanceStagedQualifiedAuthority, Error> {
    let common = &input.authority;
    let action_sha = admit_authority(common)?;
    let action_time = text(common.action, "reportCreatedAt").ok_or(Error::Input)?;
    let result_input = NativeMaintenanceTerminalResultInput {
        family: common.family,
        registration_sha256: common.registration_sha256,
        candidate_report_sha256: input.candidate_report_sha256,
        action_identity_sha256: Some(&action_sha),
        disposition: NativeMaintenanceTerminalResultDisposition::Promoted,
        next_confirmation_eligible_at: input.next_confirmation_eligible_at,
        now: common.now,
    };
    admit_result(result_input, &action_time)?;
    // require one typed promotion identity and the original deployed compare-and-swap source
    if text(common.action, "actionKind").as_deref() != Some("promote")
        || text(common.action, "family").as_deref() != Some(common.family.wire())
        || text(common.action, "expectedSourceCommit").as_deref() != Some(&common.current.commit)
        || text(common.action, "expectedSourceRelease").as_deref() != Some(&common.current.release)
        || text(common.action, "expectedSettingsSha256").as_deref()
            != Some(&common.current.settings_sha256)
        || text(common.action, "fencingToken").as_deref() != Some(&input.lease.fencing_token)
        || text(common.action, "policyReportSha256").as_deref()
            != Some(input.candidate_report_sha256)
    {
        return Err(Error::Identity);
    }
    let authority = object(vec![
        ("action", common.action.clone()),
        ("anchor", common.anchor.clone()),
        ("current", current_value(common.current)),
        ("family", string(common.family.wire())),
        ("finalizationProof", common.finalization_proof.clone()),
        ("registrationSha256", string(common.registration_sha256)),
        ("seal", common.seal.clone()),
    ]);
    let mut result = None;
    journal.mutate_checked::<Error>(
        &mut NativeWirePrimitives,
        &NativeWirePrimitives,
        |records| {
            let active = trusted_clock(records, common.now, false)?;
            let (registration, complete) =
                complete_member(records, common.family, common.registration_sha256)?;
            let due_key = registration.due_key.as_deref().ok_or(Error::Member)?;
            let release = active.get("release").ok_or(Error::Fence)?;
            // bind the actual live global release fence and complete member before staging anything
            if due_key != input.lease.due_key
                || text(&release.payload, "dueKey").as_deref() != Some(due_key)
                || text(&release.payload, "runId").as_deref() != Some(&input.lease.run_id)
                || text(&release.payload, "fencingToken").as_deref()
                    != Some(&input.lease.fencing_token)
                || field(common.action, "candidateSha256")
                    != field(&registration.payload, "candidateSha256")
                || field(common.action, "fullMemberRootSha256")
                    != field(&complete.payload, "fullMemberRootSha256")
            {
                return Err(Error::Fence);
            }
            require_unique_action(records, common.registration_sha256, Some(&action_sha))?;
            // prohibit an already used fencing identity from naming a different model action
            for record in records {
                if text(&record.payload, "fencingToken").as_deref()
                    != Some(&input.lease.fencing_token)
                {
                    continue;
                }
                if (kind(record, PREPARED)
                    && text(&record.payload, "actionIdentitySha256").as_deref()
                        != Some(&action_sha))
                    || (kind(record, "model_release_prepared")
                        && (text(&record.payload, "actionSha256").as_deref() != Some(&action_sha)
                            || text(&record.payload, "family").as_deref()
                                != Some(common.family.wire())))
                {
                    return Err(Error::Fence);
                }
            }
            let local_result = result_payload(registration, complete, result_input)?;
            let prepared = merge(
                &local_result,
                vec![
                    ("actionState", string("release_pending")),
                    ("fencingToken", string(&input.lease.fencing_token)),
                    ("actionManifestSha256", string(&action_sha)),
                    ("releaseDueKey", string(due_key)),
                    ("releaseRunId", string(&input.lease.run_id)),
                ],
            )?;
            let prefix = format!(
                "confirmation/{}/{}",
                common.family.wire(),
                common.registration_sha256
            );
            let mut additions = Vec::new();
            // all source events use exact logical coordinates in one journal mutation
            for event in [
                result_event(complete, result_input, local_result.clone())?,
                addition(
                    due_key,
                    PREPARED,
                    format!("{prefix}/action/{action_sha}/prepared"),
                    common.now,
                    prepared,
                ),
                addition(
                    due_key,
                    QUALIFIED_AUTHORITY,
                    format!("{prefix}/qualified-authority"),
                    common.now,
                    authority.clone(),
                ),
            ] {
                append_exact(records, &mut additions, event)?;
            }
            result = Some(NativeMaintenanceStagedQualifiedAuthority {
                authority: authority.clone(),
                local_result,
            });
            Ok(additions)
        },
    )?;
    result.ok_or(Error::Member)
}

/// Retain the source exact no-action C/T/F documents before installing root authority.
pub fn stage_native_maintenance_no_action_terminal_authority(
    journal: &mut NativeMaintenanceJournalStore,
    input: NativeMaintenanceTerminalAuthorityInput<'_>,
    disposition: NativeMaintenanceTerminalResultDisposition,
) -> Result<JsonValue, Error> {
    admit_authority(&input)?;
    // the source only stages evaluated rejection or support-failure authority here
    if !matches!(
        disposition,
        NativeMaintenanceTerminalResultDisposition::Rejected
            | NativeMaintenanceTerminalResultDisposition::SupportFailed
    ) || text(input.action, "contractVersion").as_deref()
        != Some("adjustment-terminal-no-action-identity/v3")
        || text(input.action, "disposition").as_deref() != Some(disposition.wire())
        || text(input.seal, "sourceCommit").as_deref() != Some(&input.current.commit)
    {
        return Err(Error::Identity);
    }
    let payload = object(vec![
        ("actionIdentity", input.action.clone()),
        ("anchor", input.anchor.clone()),
        ("current", current_value(input.current)),
        ("disposition", string(disposition.wire())),
        ("family", string(input.family.wire())),
        ("finalizationProof", input.finalization_proof.clone()),
        ("registrationSha256", string(input.registration_sha256)),
        ("seal", input.seal.clone()),
    ]);
    journal.mutate_checked::<Error>(
        &mut NativeWirePrimitives,
        &NativeWirePrimitives,
        |records| {
            trusted_clock(records, input.now, false)?;
            let (_, complete) = complete_member(records, input.family, input.registration_sha256)?;
            // allow only a still-unscored member or the exact retained same endpoint on retry
            if let Some(result) = find(records, input.registration_sha256, RESULT)
                && text(&result.payload, "disposition").as_deref() != Some(disposition.wire())
            {
                return Err(Error::Collision);
            }
            let due_key = complete.due_key.as_deref().ok_or(Error::Member)?;
            let mut additions = Vec::new();
            append_exact(
                records,
                &mut additions,
                addition(
                    due_key,
                    NO_ACTION_AUTHORITY,
                    format!(
                        "confirmation/{}/{}/no-action-authority",
                        input.family.wire(),
                        input.registration_sha256
                    ),
                    input.now,
                    payload.clone(),
                ),
            )?;
            Ok(additions)
        },
    )?;
    Ok(payload)
}

/// Read the exact family owner tombstone for native record request lineage.
pub fn read_native_maintenance_owner_terminal_tombstone(
    journal: &mut NativeMaintenanceJournalStore,
    family: NativeMaintenanceCaptureFamily,
) -> Result<Option<JsonValue>, Error> {
    let records = journal.read_verified(&mut NativeWirePrimitives, &NativeWirePrimitives)?;
    // select only the most recent actual owner-authenticated tombstone pointer for this family
    let value = records.iter().rev().find(|record| {
        kind(record, "confirmation_owner_terminal_tombstone")
            && text(&record.payload, "family").as_deref() == Some(family.wire())
    });
    value
        .map(|record| {
            field(&record.payload, "tombstone")
                .cloned()
                .ok_or(Error::Member)
        })
        .transpose()
}

/// Retain exact prior source authority by registration, never selecting a replacement live source.
pub fn read_native_maintenance_terminal_authority(
    journal: &mut NativeMaintenanceJournalStore,
    registration_sha256: &str,
    qualified: bool,
) -> Result<Option<JsonValue>, Error> {
    if !valid_hash(registration_sha256) {
        return Err(Error::Input);
    }
    let records = journal.read_verified(&mut NativeWirePrimitives, &NativeWirePrimitives)?;
    Ok(find(
        &records,
        registration_sha256,
        if qualified {
            QUALIFIED_AUTHORITY
        } else {
            NO_ACTION_AUTHORITY
        },
    )
    .map(|record| record.payload.clone()))
}

/// Validate immutable result identities and original seven-day embargo before mutation.
fn admit_result(
    input: NativeMaintenanceTerminalResultInput<'_>,
    embargo_start: &str,
) -> Result<(), Error> {
    if !valid_hash(input.registration_sha256)
        || !valid_hash(input.candidate_report_sha256)
        || input.action_identity_sha256.is_some_and(|v| !valid_hash(v))
        || (input.disposition == NativeMaintenanceTerminalResultDisposition::Promoted)
            != input.action_identity_sha256.is_some()
    {
        return Err(Error::Input);
    }
    let earliest = instant(embargo_start)?
        .checked_add(7 * DAY)
        .ok_or(Error::Clock)?;
    instant(input.now)?;
    if instant(input.next_confirmation_eligible_at)? < earliest {
        return Err(Error::Clock);
    }
    Ok(())
}

/// Rebind all staged root documents to one exact action and immutable seal.
fn admit_authority(input: &NativeMaintenanceTerminalAuthorityInput<'_>) -> Result<String, Error> {
    if !valid_hash(input.registration_sha256) || input.current.family != input.family.wire() {
        return Err(Error::Input);
    }
    instant(input.now)?;
    let action = identity(input.action)?;
    if text(input.anchor, "actionSha256").as_deref() != Some(&action)
        || text(input.anchor, "inputSealSha256").as_deref() != Some(&identity(input.seal)?)
        || text(input.finalization_proof, "actionSha256").as_deref() != Some(&action)
        || text(input.finalization_proof, "transferredAnchorSha256").as_deref()
            != Some(&identity(input.anchor)?)
        || field(input.finalization_proof, "graphManifestSha256")
            != field(input.seal, "graphManifestSha256")
    {
        return Err(Error::Identity);
    }
    Ok(action)
}

/// Find the source complete member and its own exact preregistered family slot.
fn complete_member<'a>(
    records: &'a [NativeMaintenanceJournalRecord],
    family: NativeMaintenanceCaptureFamily,
    registration_sha256: &str,
) -> Result<
    (
        &'a NativeMaintenanceJournalRecord,
        &'a NativeMaintenanceJournalRecord,
    ),
    Error,
> {
    let registration = find(records, registration_sha256, REGISTERED).ok_or(Error::Member)?;
    let complete = find(records, registration_sha256, COMPLETE).ok_or(Error::Member)?;
    if text(&registration.payload, "family").as_deref() != Some(family.wire())
        || registration.due_key != complete.due_key
        || !text(&complete.payload, "fullMemberRootSha256").is_some_and(|value| valid_hash(&value))
    {
        return Err(Error::Member);
    }
    Ok((registration, complete))
}

/// Preserve logical action ownership across all source registrations.
fn require_unique_action(
    records: &[NativeMaintenanceJournalRecord],
    registration: &str,
    action: Option<&str>,
) -> Result<(), Error> {
    if let Some(action) = action
        && records.iter().any(|record| {
            kind(record, RESULT)
                && text(&record.payload, "actionIdentitySha256").as_deref() == Some(action)
                && text(&record.payload, "registrationSha256").as_deref() != Some(registration)
        })
    {
        return Err(Error::Identity);
    }
    Ok(())
}

/// Build the source result payload by exact lifecycle-context spread.
fn result_payload(
    registration: &NativeMaintenanceJournalRecord,
    complete: &NativeMaintenanceJournalRecord,
    input: NativeMaintenanceTerminalResultInput<'_>,
) -> Result<JsonValue, Error> {
    const CONTEXT: &[&str] = &[
        "contractVersion",
        "family",
        "candidateKind",
        "candidateSha256",
        "cohortLineageSha256",
        "sourceLineageSha256",
        "reservedKeySha256",
        "intervalStartLocalDate",
        "intervalEndExclusiveLocalDate",
        "firstTargetAt",
        "terminalAccessAt",
        "gateManifestSha256",
        "inputHeadSha256",
        "registrationSha256",
    ];
    let mut payload = Vec::with_capacity(21);
    // bind every original registration context coordinate
    for key in CONTEXT {
        payload.push((
            *key,
            field(&registration.payload, key)
                .ok_or(Error::Member)?
                .clone(),
        ));
    }
    payload.extend([
        (
            "candidateReportSha256",
            string(input.candidate_report_sha256),
        ),
        (
            "actionIdentitySha256",
            input
                .action_identity_sha256
                .map(string)
                .unwrap_or(JsonValue::Null),
        ),
        ("accessState", string("opened")),
        (
            "actionState",
            string(
                if input.disposition == NativeMaintenanceTerminalResultDisposition::Promoted {
                    "action_pending"
                } else {
                    "terminal_no_action"
                },
            ),
        ),
        (
            "fullMemberRootSha256",
            field(&complete.payload, "fullMemberRootSha256")
                .ok_or(Error::Member)?
                .clone(),
        ),
        ("disposition", string(input.disposition.wire())),
        (
            "nextConfirmationEligibleAt",
            string(input.next_confirmation_eligible_at),
        ),
    ]);
    Ok(object(payload))
}

/// Retain the exact source terminal result coordinate.
fn result_event(
    complete: &NativeMaintenanceJournalRecord,
    input: NativeMaintenanceTerminalResultInput<'_>,
    payload: JsonValue,
) -> Result<NativeMaintenanceOwnedJournalAddition, Error> {
    Ok(addition(
        complete.due_key.as_deref().ok_or(Error::Member)?,
        RESULT,
        format!(
            "confirmation/{}/{}/result",
            input.family.wire(),
            input.registration_sha256
        ),
        input.now,
        payload,
    ))
}

/// Only an exact canonical payload is idempotently reusable under a logical event key.
fn append_exact(
    records: &[NativeMaintenanceJournalRecord],
    additions: &mut Vec<NativeMaintenanceOwnedJournalAddition>,
    event: NativeMaintenanceOwnedJournalAddition,
) -> Result<(), Error> {
    if let Some(previous) = records
        .iter()
        .find(|record| record.record_key == event.record_key)
    {
        if !kind(previous, &event.kind)
            || previous.due_key != event.due_key
            || previous.payload_sha256 != identity(&event.payload)?
        {
            return Err(Error::Collision);
        }
        return Ok(());
    }
    additions.push(event);
    Ok(())
}

/// Project the verified root family current result in the exact source wire shape.
fn current_value(current: &NativeFamilyReleaseCurrent) -> JsonValue {
    let nullable = |value: &Option<String>| value.as_deref().map(string).unwrap_or(JsonValue::Null);
    object(vec![
        (
            "activeInstalledReceiptSha256",
            nullable(&current.active_installed_receipt_sha256),
        ),
        ("catalogSha256", string(&current.catalog_sha256)),
        ("commit", string(&current.commit)),
        (
            "controlInstalledReceiptSha256",
            nullable(&current.control_installed_receipt_sha256),
        ),
        ("contractVersion", string(&current.contract_version)),
        ("family", string(&current.family)),
        ("release", string(&current.release)),
        ("settingsSha256", string(&current.settings_sha256)),
        (
            "shadowInstalledReceiptSha256",
            nullable(&current.shadow_installed_receipt_sha256),
        ),
        (
            "sourceServerImageDigest",
            string(&current.source_server_image_digest),
        ),
    ])
}

/// Apply source object spread semantics without duplicate field keys.
fn merge(value: &JsonValue, updates: Vec<(&str, JsonValue)>) -> Result<JsonValue, Error> {
    let JsonValue::Object(mut result) = value.clone() else {
        return Err(Error::Input);
    };
    for (key, value) in updates {
        let units = key.encode_utf16().collect::<Vec<_>>();
        if let Some((_, existing)) = result.iter_mut().find(|(name, _)| *name == units) {
            *existing = value;
        } else {
            result.push((units, value));
        }
    }
    Ok(JsonValue::Object(result))
}

/// Select only the exact original event and registration identity.
fn find<'a>(
    records: &'a [NativeMaintenanceJournalRecord],
    registration: &str,
    event: &str,
) -> Option<&'a NativeMaintenanceJournalRecord> {
    records.iter().find(|record| {
        kind(record, event)
            && text(&record.payload, "registrationSha256").as_deref() == Some(registration)
    })
}

/// Compare a source journal discriminant.
fn kind(record: &NativeMaintenanceJournalRecord, event: &str) -> bool {
    matches!(&record.kind, JsonValue::String(value) if value.iter().copied().eq(event.encode_utf16()))
}

/// Hash canonical source bytes.
fn identity(value: &JsonValue) -> Result<String, Error> {
    let bytes = NativeWirePrimitives
        .encode_sorted_json(value)
        .ok_or(Error::Input)?;
    Ok(format_digest(RevisionArchiveCanonicalCodec::sha256(
        &NativeWirePrimitives,
        &bytes,
    )))
}

/// Parse the exact canonical source UTC instant.
fn instant(value: &str) -> Result<i64, Error> {
    NativeWirePrimitives
        .canonical_utc_milliseconds(value)
        .ok_or(Error::Clock)
}

/// Exact already published qualified release mapping and original root apply request.
pub struct NativeMaintenanceQualifiedReleaseTransactionInput<'a> {
    pub family: NativeMaintenanceCaptureFamily,
    pub registration_sha256: &'a str,
    pub target: &'a crate::NativeMaintenancePublishedModelRelease,
    pub target_action: &'a JsonValue,
    pub compensation: &'a crate::NativeMaintenancePublishedModelRelease,
    pub compensation_action: &'a JsonValue,
    pub finalization_proof: &'a JsonValue,
    pub request: &'a crate::NativeMaintenanceFamilyReleaseApplyRequest,
    pub now: &'a str,
}

/// Persist the exact paired immutable release and root apply request for qualified recovery.
pub fn record_native_maintenance_qualified_release_transaction(
    journal: &mut NativeMaintenanceJournalStore,
    input: NativeMaintenanceQualifiedReleaseTransactionInput<'_>,
) -> Result<JsonValue, Error> {
    let target_action = identity(input.target_action)?;
    let compensation_action = identity(input.compensation_action)?;
    let args = input.request.arguments();
    let fence = text(input.target_action, "fencingToken").ok_or(Error::Input)?;
    // bind each root request operand to the exact staged and published canonical action
    if target_action != input.target.action_sha256
        || compensation_action != input.compensation.action_sha256
        || args[0] != input.target.release_tag
        || args[1] != input.compensation.release_tag
        || args[5] != input.family.wire()
        || args[6] != target_action
        || args[8] != fence
        || text(input.target_action, "expectedSourceRelease").as_deref() != Some(args[2])
        || text(input.target_action, "expectedSourceRelease").as_deref() != Some(args[3])
        || text(input.target_action, "expectedSettingsSha256").as_deref() != Some(args[4])
        || text(input.target_action, "policyReportSha256").as_deref() != Some(args[7])
        || text(input.finalization_proof, "actionSha256").as_deref() != Some(&target_action)
    {
        return Err(Error::Identity);
    }
    let release = |release: &crate::NativeMaintenancePublishedModelRelease| {
        object(vec![
            ("actionSha256", string(&release.action_sha256)),
            ("commitSha", string(&release.commit_sha)),
            ("releaseTag", string(&release.release_tag)),
        ])
    };
    let request = object(vec![
        ("targetRelease", string(args[0])),
        ("compensatingRelease", string(args[1])),
        ("expectedCurrentRelease", string(args[2])),
        ("expectedSourceRelease", string(args[3])),
        ("expectedSettingsSha256", string(args[4])),
        ("family", string(args[5])),
        ("actionSha256", string(args[6])),
        ("reportSha256", string(args[7])),
        ("fencingToken", string(args[8])),
    ]);
    let payload = object(vec![
        ("compensation", release(input.compensation)),
        ("compensationAction", input.compensation_action.clone()),
        ("family", string(input.family.wire())),
        ("finalizationProof", input.finalization_proof.clone()),
        ("registrationSha256", string(input.registration_sha256)),
        ("releaseRequest", request),
        ("target", release(input.target)),
        ("targetAction", input.target_action.clone()),
    ]);
    journal.mutate_checked::<Error>(
        &mut NativeWirePrimitives,
        &NativeWirePrimitives,
        |records| {
            trusted_clock(records, input.now, false)?;
            let prepared =
                find(records, input.registration_sha256, PREPARED).ok_or(Error::Member)?;
            let pair = records
                .iter()
                .find(|record| {
                    kind(record, "model_release_pair_prepared")
                        && text(&record.payload, "targetActionSha256").as_deref()
                            == Some(&target_action)
                        && text(&record.payload, "compensationActionSha256").as_deref()
                            == Some(&compensation_action)
                })
                .ok_or(Error::Member)?;
            // require the exact publication pair and matching independently prepared confirmation action
            if text(&prepared.payload, "actionIdentitySha256").as_deref() != Some(&target_action)
                || text(&prepared.payload, "actionManifestSha256").as_deref()
                    != Some(&target_action)
                || text(&prepared.payload, "fencingToken").as_deref() != Some(&fence)
                || text(&pair.payload, "family").as_deref() != Some(input.family.wire())
                || text(&pair.payload, "fencingToken").as_deref() != Some(&fence)
                || text(&pair.payload, "targetCommitSha").as_deref()
                    != Some(&input.target.commit_sha)
                || text(&pair.payload, "targetReleaseTag").as_deref()
                    != Some(&input.target.release_tag)
                || text(&pair.payload, "compensationCommitSha").as_deref()
                    != Some(&input.compensation.commit_sha)
                || text(&pair.payload, "compensationReleaseTag").as_deref()
                    != Some(&input.compensation.release_tag)
            {
                return Err(Error::Identity);
            }
            let due = prepared.due_key.as_deref().ok_or(Error::Member)?;
            let mut additions = Vec::new();
            append_exact(
                records,
                &mut additions,
                addition(
                    due,
                    "qualified_model_release_transaction",
                    format!("model-release/{target_action}/qualified-transaction"),
                    input.now,
                    payload.clone(),
                ),
            )?;
            Ok(additions)
        },
    )?;
    Ok(payload)
}

/// Read the original qualified recovery transaction after a process interruption.
pub fn read_native_maintenance_qualified_release_transaction(
    journal: &mut NativeMaintenanceJournalStore,
    registration_sha256: &str,
) -> Result<Option<JsonValue>, Error> {
    if !valid_hash(registration_sha256) {
        return Err(Error::Input);
    }
    let records = journal.read_verified(&mut NativeWirePrimitives, &NativeWirePrimitives)?;
    Ok(find(
        &records,
        registration_sha256,
        "qualified_model_release_transaction",
    )
    .map(|record| record.payload.clone()))
}

/// Closed qualified safe-state compensation packages.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceQualifiedCompensationKind {
    Raw,
    Incumbent,
}

/// Exact remote root outcome and the already published immutable paired compensation.
pub struct NativeMaintenanceQualifiedActionOutcomeInput<'a> {
    pub family: NativeMaintenanceCaptureFamily,
    pub registration_sha256: &'a str,
    pub action_sha256: &'a str,
    pub compensation_action_sha256: &'a str,
    pub policy_report_sha256: &'a str,
    pub fencing_token: &'a str,
    pub status: &'a crate::NativeMaintenanceFamilyReleaseApplyStatus,
    pub compensation_kind: NativeMaintenanceQualifiedCompensationKind,
    pub now: &'a str,
}

/// Persist the actual terminal root apply, verification, and acknowledgement/compensation stages.
pub fn record_native_maintenance_qualified_action_outcome(
    journal: &mut NativeMaintenanceJournalStore,
    input: NativeMaintenanceQualifiedActionOutcomeInput<'_>,
) -> Result<bool, Error> {
    use crate::{
        NativeMaintenanceFamilyReleaseApplyState as State,
        NativeMaintenanceFamilyReleaseCompensationState as Compensation,
        NativeMaintenanceFamilyReleaseOutcome as Outcome,
    };
    let status = input.status;
    // retain only exact same-family and same-fence authenticated root results
    if ![
        input.registration_sha256,
        input.action_sha256,
        input.compensation_action_sha256,
        input.policy_report_sha256,
    ]
    .into_iter()
    .all(valid_hash)
        || input.action_sha256 == input.compensation_action_sha256
        || status.action_sha256 != input.action_sha256
        || status.fencing_token != input.fencing_token
        || status
            .family
            .as_deref()
            .is_some_and(|value| value != input.family.wire())
        || NativeWirePrimitives
            .encode_sorted_json(&status.value)
            .as_deref()
            != Some(&status.canonical_bytes)
    {
        return Err(Error::Identity);
    }
    instant(input.now)?;
    let receipt = format_digest(RevisionArchiveCanonicalCodec::sha256(
        &NativeWirePrimitives,
        &status.canonical_bytes,
    ));
    let (verification, acknowledgement, compensation, serving_changed) = match status.state {
        State::OperatorOffUnapplied => {
            (None, None, Some(("unapplied", "expired_unapplied")), false)
        }
        State::Acknowledged if status.outcome == Some(Outcome::Active) => {
            (Some("verified"), Some("active"), None, true)
        }
        State::Acknowledged if status.outcome == Some(Outcome::DeployedOperatorOff) => (
            Some("deployed_operator_off"),
            Some("deployed_operator_off"),
            None,
            false,
        ),
        State::CompensationRequired if status.compensation_state == Compensation::Verified => {
            let closed = match input.compensation_kind {
                NativeMaintenanceQualifiedCompensationKind::Raw => ("raw_installed", "raw"),
                NativeMaintenanceQualifiedCompensationKind::Incumbent => {
                    ("incumbent_restored", "failed")
                }
            };
            (Some("failed"), None, Some(closed), false)
        }
        _ => return Err(Error::Identity),
    };
    journal.mutate_checked::<Error>(
        &mut NativeWirePrimitives,
        &NativeWirePrimitives,
        |records| {
            let result = find(records, input.registration_sha256, RESULT).ok_or(Error::Member)?;
            let prepared =
                find(records, input.registration_sha256, PREPARED).ok_or(Error::Member)?;
            let transaction = find(
                records,
                input.registration_sha256,
                "qualified_model_release_transaction",
            )
            .ok_or(Error::Member)?;
            // bind the root response to the original prepared exact release pair
            if text(&result.payload, "family").as_deref() != Some(input.family.wire())
                || text(&result.payload, "disposition").as_deref() != Some("promoted")
                || text(&result.payload, "actionIdentitySha256").as_deref()
                    != Some(input.action_sha256)
                || text(&prepared.payload, "fencingToken").as_deref() != Some(input.fencing_token)
                || text(&result.payload, "candidateReportSha256").as_deref()
                    != Some(input.policy_report_sha256)
                || field(&transaction.payload, "compensation")
                    .and_then(|v| text(v, "actionSha256"))
                    .as_deref()
                    != Some(input.compensation_action_sha256)
                || field(&transaction.payload, "target")
                    .and_then(|v| text(v, "releaseTag"))
                    .as_deref()
                    != Some(&status.target_release)
                || field(&transaction.payload, "compensation")
                    .and_then(|v| text(v, "releaseTag"))
                    .as_deref()
                    != Some(&status.compensating_release)
            {
                return Err(Error::Identity);
            }
            let due = prepared.due_key.as_deref().ok_or(Error::Member)?;
            let prefix = format!(
                "confirmation/{}/{}/action/{}",
                input.family.wire(),
                input.registration_sha256,
                input.action_sha256
            );
            let action_context = |state: &str, fields: Vec<(&str, JsonValue)>| {
                let mut updates = vec![
                    ("actionState", string(state)),
                    ("fencingToken", string(input.fencing_token)),
                ];
                updates.extend(fields);
                merge(&result.payload, updates)
            };
            let mut additions = Vec::new();
            let mut add = |kind: &str, suffix: &str, payload: JsonValue| -> Result<(), Error> {
                // permit only an exact retained historical stage after durable reconciliation
                if find(records, input.registration_sha256, kind).is_none()
                    && (find(
                        records,
                        input.registration_sha256,
                        "confirmation_action_acknowledged",
                    )
                    .is_some()
                        || find(
                            records,
                            input.registration_sha256,
                            "confirmation_action_compensation_result",
                        )
                        .is_some())
                {
                    return Err(Error::Collision);
                }
                append_exact(
                    records,
                    &mut additions,
                    addition(due, kind, format!("{prefix}/{suffix}"), input.now, payload),
                )
            };
            // retain the exact source applied then verified stages only for an actual apply
            if let Some(verification) = verification {
                add(
                    "confirmation_action_applied",
                    "applied",
                    action_context(
                        "deploying",
                        vec![
                            ("actionManifestSha256", string(input.action_sha256)),
                            ("applyReceiptSha256", string(&receipt)),
                        ],
                    )?,
                )?;
                add(
                    "confirmation_action_verified",
                    "verified",
                    action_context(
                        if verification == "failed" {
                            "failed"
                        } else {
                            "deploying"
                        },
                        vec![
                            ("applyReceiptSha256", string(&receipt)),
                            ("verificationOutcome", string(verification)),
                            ("verificationSha256", string(&receipt)),
                        ],
                    )?,
                )?;
            } else if find(
                records,
                input.registration_sha256,
                "confirmation_action_applied",
            )
            .is_some()
            {
                return Err(Error::Collision);
            }
            // close either acknowledged serving application or verified safe compensation once
            if let Some(outcome) = acknowledgement {
                add(
                    "confirmation_action_acknowledged",
                    "acknowledged",
                    action_context(
                        outcome,
                        vec![
                            ("acknowledgementSha256", string(&receipt)),
                            ("outcome", string(outcome)),
                            ("verificationSha256", string(&receipt)),
                        ],
                    )?,
                )?;
            } else if let Some((outcome, state)) = compensation {
                if records.iter().any(|record| {
                    (kind(record, RESULT)
                        && text(&record.payload, "actionIdentitySha256").as_deref()
                            == Some(input.compensation_action_sha256))
                        || (kind(record, "confirmation_action_compensation_result")
                            && text(&record.payload, "compensationActionIdentitySha256").as_deref()
                                == Some(input.compensation_action_sha256)
                            && text(&record.payload, "registrationSha256").as_deref()
                                != Some(input.registration_sha256))
                }) {
                    return Err(Error::Collision);
                }
                add(
                    "confirmation_action_compensation_result",
                    "compensation-result",
                    action_context(
                        state,
                        vec![
                            (
                                "compensationActionIdentitySha256",
                                string(input.compensation_action_sha256),
                            ),
                            (
                                "compensationReportSha256",
                                string(input.policy_report_sha256),
                            ),
                            ("outcome", string(outcome)),
                        ],
                    )?,
                )?;
            }
            Ok(additions)
        },
    )?;
    Ok(serving_changed)
}

/// Read the exact terminal result retained with a staged C/T/F authority.
pub fn read_native_maintenance_confirmation_result(
    journal: &mut NativeMaintenanceJournalStore,
    registration_sha256: &str,
) -> Result<Option<JsonValue>, Error> {
    if !valid_hash(registration_sha256) {
        return Err(Error::Input);
    }
    let records = journal.read_verified(&mut NativeWirePrimitives, &NativeWirePrimitives)?;
    Ok(find(&records, registration_sha256, RESULT).map(|record| record.payload.clone()))
}

/// Continue only the exact original fenced qualified action, never allocating a new fence.
pub fn continue_native_maintenance_qualified_release_lease(
    journal: &mut NativeMaintenanceJournalStore,
    family: NativeMaintenanceCaptureFamily,
    registration_sha256: &str,
    now: &str,
) -> Result<Option<NativeMaintenanceReleaseLease>, Error> {
    if !valid_hash(registration_sha256) {
        return Err(Error::Input);
    }
    let now_ms = instant(now)?;
    let mut result = None;
    journal.mutate_checked::<Error>(
        &mut NativeWirePrimitives,
        &NativeWirePrimitives,
        |records| {
            let active = trusted_clock(records, now, true)?;
            let prepared = find(records, registration_sha256, PREPARED).ok_or(Error::Member)?;
            let authority =
                find(records, registration_sha256, QUALIFIED_AUTHORITY).ok_or(Error::Member)?;
            // admit only the exact previously prepared action and its staged authority
            if text(&prepared.payload, "family").as_deref() != Some(family.wire())
                || text(&authority.payload, "family").as_deref() != Some(family.wire())
                || text(&prepared.payload, "actionIdentitySha256").as_deref()
                    != Some(&identity(
                        field(&authority.payload, "action").ok_or(Error::Member)?,
                    )?)
            {
                return Err(Error::Fence);
            }
            let Some(lease) = active.get("release") else {
                // permit owner-only recovery once the original local serving outcome is terminal
                let records = records.iter().collect::<Vec<_>>();
                if !crate::native_maintenance_confirmation_journal::is_terminal(
                    &records,
                    registration_sha256,
                ) {
                    return Err(Error::Fence);
                }
                return Ok(Vec::new());
            };
            // refuse a different release owner rather than adopting a new fence
            if text(&lease.payload, "scope").as_deref() != Some("release")
                || field(&lease.payload, "dueKey") != field(&prepared.payload, "releaseDueKey")
                || field(&lease.payload, "runId") != field(&prepared.payload, "releaseRunId")
                || field(&lease.payload, "fencingToken") != field(&prepared.payload, "fencingToken")
            {
                return Err(Error::Fence);
            }
            let required = |key| text(&lease.payload, key).ok_or(Error::Fence);
            let mut projected = NativeMaintenanceReleaseLease {
                acquired_at: required("acquiredAt")?,
                due_key: required("dueKey")?,
                expires_at: required("expiresAt")?,
                fencing_token: required("fencingToken")?,
                input_head_sha256: required("inputHeadSha256")?,
                run_id: required("runId")?,
            };
            if instant(&projected.expires_at)? > now_ms {
                result = Some(projected);
                return Ok(Vec::new());
            }
            // replicate the original eight-hour same-fence continuation from the expired record
            projected.acquired_at = now.to_owned();
            projected.expires_at = NativeWirePrimitives
                .format_utc_milliseconds(
                    now_ms
                        .checked_add(8 * 60 * 60 * 1_000)
                        .ok_or(Error::Clock)?,
                )
                .ok_or(Error::Clock)?;
            let payload = object(vec![
                ("acquiredAt", string(&projected.acquired_at)),
                ("dueKey", string(&projected.due_key)),
                ("expiresAt", string(&projected.expires_at)),
                ("fencingToken", string(&projected.fencing_token)),
                ("inputHeadSha256", string(&projected.input_head_sha256)),
                ("predecessorLeaseRecordSha256", string(&lease.record_sha256)),
                ("runId", string(&projected.run_id)),
                ("scope", string("release")),
            ]);
            let event = addition(
                &projected.due_key,
                "lease_continued",
                format!(
                    "lease/release/{}/continued/{}",
                    projected.run_id, lease.record_sha256
                ),
                now,
                payload,
            );
            result = Some(projected);
            Ok(vec![event])
        },
    )?;
    Ok(result)
}
