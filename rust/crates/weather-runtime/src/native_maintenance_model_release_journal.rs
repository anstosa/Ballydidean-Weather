//! Fenced release lease and immutable model-release receipts over the verified native journal.

use std::cell::RefCell;
use std::fmt::{self, Display, Formatter};

use weather_contracts::JsonValue;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};

use crate::maintenance_canonical_json::{field, text, valid_hash};
use crate::native_maintenance_archive_journal::{addition, due, object, string, trusted_clock};
use crate::native_maintenance_confirmation_journal::is_terminal;
use crate::native_maintenance_retirement_files_v2::format_digest;
use crate::{
    NativeMaintenanceArchiveJournalError, NativeMaintenanceCaptureFamily,
    NativeMaintenanceJournalRecord, NativeMaintenanceJournalStore,
    NativeMaintenanceJournalStoreError, NativeMaintenanceOwnedJournalAddition,
    NativeWirePrimitives,
};

const RELEASE_SCOPE: &str = "release";
const RELEASE_LEASE_MILLISECONDS: i64 = 8 * 60 * 60 * 1_000;

/// Exact immutable release-lease acquisition material.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceReleaseLeaseInput<'a> {
    pub due_key: &'a str,
    pub input_head_sha256: &'a str,
    pub now: &'a str,
    pub run_id: &'a str,
}

/// One live source release fence retained by the durable journal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceReleaseLease {
    pub acquired_at: String,
    pub due_key: String,
    pub expires_at: String,
    pub fencing_token: String,
    pub input_head_sha256: String,
    pub run_id: String,
}

/// Exact acquisition outcome including the source's terminal-due no-op.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceReleaseLeaseAcquisition {
    Held(NativeMaintenanceReleaseLease),
    AlreadyComplete { due_key: String },
}

/// Exact live release-lease release material.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceReleaseLeaseReleaseInput<'a> {
    pub due_key: &'a str,
    pub now: &'a str,
    pub run_id: &'a str,
}

/// Exact target-release preparation fields before any remote ref publication.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceModelReleasePrepareInput<'a> {
    pub action_sha256: &'a str,
    pub action_tag: &'a str,
    pub branch: &'a str,
    pub commit_sha: &'a str,
    pub expected_source_commit: &'a str,
    pub family: NativeMaintenanceCaptureFamily,
    pub fencing_token: &'a str,
    pub now: &'a str,
    pub package_root_sha256: &'a str,
    pub release_due_key: &'a str,
    pub release_run_id: &'a str,
    pub release_tag: &'a str,
}

/// Exact compensating-release preparation fields bound to one target action.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceCompensatingModelReleasePrepareInput<'a> {
    pub action_sha256: &'a str,
    pub action_tag: &'a str,
    pub branch: &'a str,
    pub commit_sha: &'a str,
    pub expected_source_commit: &'a str,
    pub family: NativeMaintenanceCaptureFamily,
    pub fencing_token: &'a str,
    pub now: &'a str,
    pub package_root_sha256: &'a str,
    pub predecessor_action_sha256: &'a str,
    pub release_due_key: &'a str,
    pub release_run_id: &'a str,
    pub release_tag: &'a str,
}

/// Exact collision lineage for one already prepared immutable release.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceModelReleaseTagCollisionInput<'a> {
    pub action_sha256: &'a str,
    pub attempted_release_tag: &'a str,
    pub collision_commit_sha: &'a str,
    pub family: NativeMaintenanceCaptureFamily,
    pub fencing_token: &'a str,
    pub now: &'a str,
    pub successor_release_tag: &'a str,
}

/// Exact published target/compensation pair before privileged family mutation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceModelReleasePairInput<'a> {
    pub compensation_action_sha256: &'a str,
    pub compensation_commit_sha: &'a str,
    pub compensation_release_tag: &'a str,
    pub family: NativeMaintenanceCaptureFamily,
    pub fencing_token: &'a str,
    pub now: &'a str,
    pub target_action_sha256: &'a str,
    pub target_commit_sha: &'a str,
    pub target_release_tag: &'a str,
}

/// Exact durable payload and logical record coordinate accepted by one journal method.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceModelReleaseJournalReceipt {
    pub kind: String,
    pub payload: JsonValue,
    pub record_key: String,
}

/// Source-compatible release journal refusal classes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceModelReleaseJournalError {
    Store(NativeMaintenanceJournalStoreError),
    Journal(NativeMaintenanceArchiveJournalError),
    Input,
    State,
    DueKeyCollision,
    LeaseOccupied,
    FencingTokenExhausted,
    UnknownLease,
    ConfirmationActionReconciliationRequired,
    ReleaseLeaseUnavailable,
    ModelReleaseFenceReused,
    ModelReleaseIdentityReused,
    ModelCompensationTargetUnavailable,
    ModelReleaseCollisionUnbound,
    ModelReleasePairUnbound,
    JournalRecordCollision,
}

impl Display for NativeMaintenanceModelReleaseJournalError {
    /// Render one stable source failure category without filesystem details.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        // retain the predecessor's operation-specific state categories
        formatter.write_str(match self {
            Self::Store(_) => "maintenance journal store failed",
            Self::Journal(_) => "maintenance journal state is invalid",
            Self::Input => "model release journal input is invalid",
            Self::State => "model release journal state is invalid",
            Self::DueKeyCollision => "due_key_collision",
            Self::LeaseOccupied => "lease_occupied",
            Self::FencingTokenExhausted => "fencing_token_exhausted",
            Self::UnknownLease => "unknown_lease",
            Self::ConfirmationActionReconciliationRequired => {
                "confirmation_action_reconciliation_required"
            }
            Self::ReleaseLeaseUnavailable => "release_lease_unavailable",
            Self::ModelReleaseFenceReused => "model_release_fence_reused",
            Self::ModelReleaseIdentityReused => "model_release_identity_reused",
            Self::ModelCompensationTargetUnavailable => "model_compensation_target_unavailable",
            Self::ModelReleaseCollisionUnbound => "model_release_collision_unbound",
            Self::ModelReleasePairUnbound => "model_release_pair_unbound",
            Self::JournalRecordCollision => "journal_record_collision",
        })
    }
}

impl std::error::Error for NativeMaintenanceModelReleaseJournalError {
    /// Preserve only typed native storage and verified-journal sources.
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        // avoid exposing any payload material through error chaining
        match self {
            Self::Store(error) => Some(error),
            Self::Journal(_) => None,
            _ => None,
        }
    }
}

impl From<NativeMaintenanceJournalStoreError> for NativeMaintenanceModelReleaseJournalError {
    /// Retain durable store failure ownership.
    fn from(error: NativeMaintenanceJournalStoreError) -> Self {
        Self::Store(error)
    }
}

impl From<NativeMaintenanceArchiveJournalError> for NativeMaintenanceModelReleaseJournalError {
    /// Retain the shared trusted-clock and active-lease refusal.
    fn from(error: NativeMaintenanceArchiveJournalError) -> Self {
        Self::Journal(error)
    }
}

/// Acquire the one global release scope and allocate its monotonic unsigned fence atomically.
pub fn acquire_native_maintenance_release_lease(
    store: &mut NativeMaintenanceJournalStore,
    input: NativeMaintenanceReleaseLeaseInput<'_>,
) -> Result<NativeMaintenanceReleaseLeaseAcquisition, NativeMaintenanceModelReleaseJournalError> {
    use NativeMaintenanceModelReleaseJournalError as Error;

    validate_release_lease_input(input)?;
    let result = RefCell::new(None);
    store.mutate_checked(
        &mut NativeWirePrimitives,
        &NativeWirePrimitives,
        |records| {
            let active = trusted_clock(records, input.now, false)?;
            let registration = due(records, input.due_key)?;
            let input_head = parse_hash(input.input_head_sha256).ok_or(Error::Input)?;
            // prohibit one due key from acquiring a different immutable input
            if registration
                .input_head_sha256
                .is_some_and(|existing| existing != input_head)
            {
                return Err(Error::DueKeyCollision);
            }
            // preserve only an exact idempotent still-live release acquisition
            if let Some(existing) = active.get(RELEASE_SCOPE) {
                let lease = release_lease(existing)?;
                if lease.run_id == input.run_id
                    && lease.due_key == input.due_key
                    && lease.input_head_sha256 == input.input_head_sha256
                {
                    result.replace(Some(NativeMaintenanceReleaseLeaseAcquisition::Held(lease)));
                    return Ok(Vec::new());
                }
                return Err(Error::LeaseOccupied);
            }
            // preserve the source no-op when this due already has a terminal output
            if registration.output_sha256.is_some() {
                result.replace(Some(
                    NativeMaintenanceReleaseLeaseAcquisition::AlreadyComplete {
                        due_key: input.due_key.to_owned(),
                    },
                ));
                return Ok(Vec::new());
            }
            let fencing_token = allocate_fencing_token(records)?.to_string();
            let acquired_at = input.now.to_owned();
            let expires_at = NativeWirePrimitives
                .format_utc_milliseconds(
                    instant(input.now)?
                        .checked_add(RELEASE_LEASE_MILLISECONDS)
                        .ok_or(Error::State)?,
                )
                .ok_or(Error::State)?;
            let lease = NativeMaintenanceReleaseLease {
                acquired_at,
                due_key: input.due_key.to_owned(),
                expires_at,
                fencing_token,
                input_head_sha256: input.input_head_sha256.to_owned(),
                run_id: input.run_id.to_owned(),
            };
            let mut additions = Vec::with_capacity(2);
            // record the immutable due input before its first lease event
            if registration.input_head_sha256.is_none() {
                additions.push(addition(
                    input.due_key,
                    "due_registered",
                    format!("due/{}", input.due_key),
                    input.now,
                    object(vec![("inputHeadSha256", string(input.input_head_sha256))]),
                ));
            }
            additions.push(addition(
                input.due_key,
                "lease_acquired",
                format!("lease/release/{}/acquired", input.run_id),
                input.now,
                release_lease_payload(&lease),
            ));
            result.replace(Some(NativeMaintenanceReleaseLeaseAcquisition::Held(lease)));
            Ok(additions)
        },
    )?;
    result.into_inner().ok_or(Error::State)
}

/// Release one exact live release lease after any prepared confirmation action is terminal.
pub fn release_native_maintenance_release_lease(
    store: &mut NativeMaintenanceJournalStore,
    input: NativeMaintenanceReleaseLeaseReleaseInput<'_>,
) -> Result<(), NativeMaintenanceModelReleaseJournalError> {
    use NativeMaintenanceModelReleaseJournalError as Error;

    // close every external coordinate before taking the durable journal lock
    if !valid_due_key(input.due_key) || !valid_run_id(input.run_id) || instant(input.now).is_err() {
        return Err(Error::Input);
    }
    store.mutate_checked(
        &mut NativeWirePrimitives,
        &NativeWirePrimitives,
        |records| {
            let active = trusted_clock(records, input.now, false)?;
            let lease = active.get(RELEASE_SCOPE).ok_or(Error::UnknownLease)?;
            let retained = release_lease(lease)?;
            // refuse an unknown or replaced release lease
            if retained.run_id != input.run_id || retained.due_key != input.due_key {
                return Err(Error::UnknownLease);
            }
            // retain its fence until the bound confirmation action reaches a durable terminal
            if has_unreconciled_prepared_action(records, &retained)? {
                return Err(Error::ConfirmationActionReconciliationRequired);
            }
            Ok(vec![addition(
                input.due_key,
                "lease_released",
                format!("lease/release/{}/released", input.run_id),
                input.now,
                object(vec![
                    ("scope", string(RELEASE_SCOPE)),
                    ("runId", string(input.run_id)),
                    ("dueKey", string(input.due_key)),
                    ("releasedAt", string(input.now)),
                ]),
            )])
        },
    )
}

/// Persist one target model-release preparation under the exact live release fence.
pub fn prepare_native_maintenance_model_release(
    store: &mut NativeMaintenanceJournalStore,
    input: NativeMaintenanceModelReleasePrepareInput<'_>,
) -> Result<NativeMaintenanceModelReleaseJournalReceipt, NativeMaintenanceModelReleaseJournalError>
{
    use NativeMaintenanceModelReleaseJournalError as Error;

    validate_model_release_input(input)?;
    let payload = model_release_payload(input);
    let receipt = NativeMaintenanceModelReleaseJournalReceipt {
        kind: "model_release_prepared".to_owned(),
        payload: payload.clone(),
        record_key: format!("model-release/{}/prepared", input.action_sha256),
    };
    store.mutate_checked(
        &mut NativeWirePrimitives,
        &NativeWirePrimitives,
        |records| {
            let active = trusted_clock(records, input.now, false)?;
            require_release_lease(
                active.get(RELEASE_SCOPE),
                input.release_due_key,
                input.release_run_id,
                input.fencing_token,
            )?;
            // cross-bind an already prepared confirmation action sharing this fence
            if let Some(confirmation) = records.iter().find(|record| {
                kind(record, "confirmation_action_prepared")
                    && text(&record.payload, "fencingToken").as_deref() == Some(input.fencing_token)
            }) && (text(&confirmation.payload, "actionManifestSha256").as_deref()
                != Some(input.action_sha256)
                || text(&confirmation.payload, "family").as_deref() != Some(input.family.wire()))
            {
                return Err(Error::ModelReleaseFenceReused);
            }
            // prohibit reuse of any immutable remote identity by another preparation
            for record in records {
                if !is_model_release_preparation(record)
                    || text(&record.payload, "actionSha256").as_deref() == Some(input.action_sha256)
                {
                    continue;
                }
                let intentional_compensation_fence =
                    kind(record, "model_compensating_release_prepared")
                        && text(&record.payload, "predecessorActionSha256").as_deref()
                            == Some(input.action_sha256);
                if field_equals(&record.payload, "branch", input.branch)
                    || field_equals(&record.payload, "actionTag", input.action_tag)
                    || field_equals(&record.payload, "releaseTag", input.release_tag)
                    || field_equals(&record.payload, "commitSha", input.commit_sha)
                    || (field_equals(&record.payload, "fencingToken", input.fencing_token)
                        && !intentional_compensation_fence)
                {
                    return Err(Error::ModelReleaseIdentityReused);
                }
            }
            unique_addition(
                records,
                addition(
                    input.release_due_key,
                    &receipt.kind,
                    receipt.record_key.clone(),
                    input.now,
                    payload.clone(),
                ),
            )
        },
    )?;
    Ok(receipt)
}

/// Persist one compensation preparation bound to the target commit and release fence.
pub fn prepare_native_maintenance_compensating_model_release(
    store: &mut NativeMaintenanceJournalStore,
    input: NativeMaintenanceCompensatingModelReleasePrepareInput<'_>,
) -> Result<NativeMaintenanceModelReleaseJournalReceipt, NativeMaintenanceModelReleaseJournalError>
{
    use NativeMaintenanceModelReleaseJournalError as Error;

    validate_compensating_release_input(input)?;
    let payload = compensating_release_payload(input);
    let receipt = NativeMaintenanceModelReleaseJournalReceipt {
        kind: "model_compensating_release_prepared".to_owned(),
        payload: payload.clone(),
        record_key: format!("model-release/{}/prepared", input.action_sha256),
    };
    store.mutate_checked(
        &mut NativeWirePrimitives,
        &NativeWirePrimitives,
        |records| {
            let active = trusted_clock(records, input.now, false)?;
            let lease = active.get(RELEASE_SCOPE);
            let target = find_model_release_preparation(records, input.predecessor_action_sha256);
            // bind the compensation base and lease to one exact prepared target
            if lease.is_none_or(|lease| {
                !field_equals(&lease.payload, "runId", input.release_run_id)
                    || !field_equals(&lease.payload, "dueKey", input.release_due_key)
                    || !field_equals(&lease.payload, "fencingToken", input.fencing_token)
            }) || target.is_none_or(|target| {
                !kind(target, "model_release_prepared")
                    || !field_equals(&target.payload, "family", input.family.wire())
                    || !field_equals(&target.payload, "fencingToken", input.fencing_token)
                    || !field_equals(&target.payload, "releaseDueKey", input.release_due_key)
                    || !field_equals(&target.payload, "releaseRunId", input.release_run_id)
                    || !field_equals(&target.payload, "commitSha", input.expected_source_commit)
            }) || input.action_sha256 == input.predecessor_action_sha256
            {
                return Err(Error::ModelCompensationTargetUnavailable);
            }
            // prohibit every remote identity alias except the intentional shared fence
            for record in records {
                if !is_model_release_preparation(record)
                    || field_equals(&record.payload, "actionSha256", input.action_sha256)
                {
                    continue;
                }
                if field_equals(&record.payload, "branch", input.branch)
                    || field_equals(&record.payload, "actionTag", input.action_tag)
                    || field_equals(&record.payload, "releaseTag", input.release_tag)
                    || field_equals(&record.payload, "commitSha", input.commit_sha)
                {
                    return Err(Error::ModelReleaseIdentityReused);
                }
            }
            unique_addition(
                records,
                addition(
                    input.release_due_key,
                    &receipt.kind,
                    receipt.record_key.clone(),
                    input.now,
                    payload.clone(),
                ),
            )
        },
    )?;
    Ok(receipt)
}

/// Persist one exact release-tag collision and its next absent successor.
pub fn record_native_maintenance_model_release_tag_collision(
    store: &mut NativeMaintenanceJournalStore,
    input: NativeMaintenanceModelReleaseTagCollisionInput<'_>,
) -> Result<NativeMaintenanceModelReleaseJournalReceipt, NativeMaintenanceModelReleaseJournalError>
{
    use NativeMaintenanceModelReleaseJournalError as Error;

    validate_collision_input(input)?;
    let payload = object(vec![
        ("actionSha256", string(input.action_sha256)),
        ("attemptedReleaseTag", string(input.attempted_release_tag)),
        ("collisionCommitSha", string(input.collision_commit_sha)),
        ("family", string(input.family.wire())),
        ("fencingToken", string(input.fencing_token)),
        ("successorReleaseTag", string(input.successor_release_tag)),
    ]);
    let receipt = NativeMaintenanceModelReleaseJournalReceipt {
        kind: "model_release_tag_collision".to_owned(),
        payload: payload.clone(),
        record_key: format!(
            "model-release/{}/collision/{}",
            input.action_sha256, input.attempted_release_tag
        ),
    };
    store.mutate_checked(
        &mut NativeWirePrimitives,
        &NativeWirePrimitives,
        |records| {
            let prepared = find_model_release_preparation(records, input.action_sha256)
                .ok_or(Error::ModelReleaseCollisionUnbound)?;
            let expected = records
                .iter()
                .rev()
                .find(|record| {
                    kind(record, "model_release_tag_collision")
                        && field_equals(&record.payload, "actionSha256", input.action_sha256)
                })
                .and_then(|record| text(&record.payload, "successorReleaseTag"))
                .or_else(|| text(&prepared.payload, "releaseTag"))
                .ok_or(Error::State)?;
            // require the exact family, fence, and attempted-tag ancestry
            if !field_equals(&prepared.payload, "family", input.family.wire())
                || !field_equals(&prepared.payload, "fencingToken", input.fencing_token)
                || expected != input.attempted_release_tag
                || input.successor_release_tag == input.attempted_release_tag
            {
                return Err(Error::ModelReleaseCollisionUnbound);
            }
            let due_key = prepared.due_key.as_deref().ok_or(Error::State)?;
            unique_addition(
                records,
                addition(
                    due_key,
                    &receipt.kind,
                    receipt.record_key.clone(),
                    input.now,
                    payload.clone(),
                ),
            )
        },
    )?;
    Ok(receipt)
}

/// Persist the collision-adjusted target and compensation release pair atomically.
pub fn record_native_maintenance_model_release_pair(
    store: &mut NativeMaintenanceJournalStore,
    input: NativeMaintenanceModelReleasePairInput<'_>,
) -> Result<NativeMaintenanceModelReleaseJournalReceipt, NativeMaintenanceModelReleaseJournalError>
{
    use NativeMaintenanceModelReleaseJournalError as Error;

    validate_pair_input(input)?;
    let payload = object(vec![
        (
            "compensationActionSha256",
            string(input.compensation_action_sha256),
        ),
        (
            "compensationCommitSha",
            string(input.compensation_commit_sha),
        ),
        (
            "compensationReleaseTag",
            string(input.compensation_release_tag),
        ),
        ("family", string(input.family.wire())),
        ("fencingToken", string(input.fencing_token)),
        ("targetActionSha256", string(input.target_action_sha256)),
        ("targetCommitSha", string(input.target_commit_sha)),
        ("targetReleaseTag", string(input.target_release_tag)),
    ]);
    let receipt = NativeMaintenanceModelReleaseJournalReceipt {
        kind: "model_release_pair_prepared".to_owned(),
        payload: payload.clone(),
        record_key: format!(
            "model-release/{}/pair/{}",
            input.target_action_sha256, input.compensation_action_sha256
        ),
    };
    store.mutate_checked(
        &mut NativeWirePrimitives,
        &NativeWirePrimitives,
        |records| {
            let active = trusted_clock(records, input.now, false)?;
            let target = find_model_release_preparation(records, input.target_action_sha256)
                .ok_or(Error::ModelReleasePairUnbound)?;
            let compensation =
                find_model_release_preparation(records, input.compensation_action_sha256)
                    .ok_or(Error::ModelReleasePairUnbound)?;
            let lease = active
                .get(RELEASE_SCOPE)
                .ok_or(Error::ModelReleasePairUnbound)?;
            let target_due = text(&target.payload, "releaseDueKey").ok_or(Error::State)?;
            let target_run = text(&target.payload, "releaseRunId").ok_or(Error::State)?;
            // close the exact target, compensation, lease, commit, family, and final tags
            if !kind(target, "model_release_prepared")
                || !kind(compensation, "model_compensating_release_prepared")
                || !field_equals(
                    &compensation.payload,
                    "predecessorActionSha256",
                    input.target_action_sha256,
                )
                || !field_equals(
                    &compensation.payload,
                    "expectedSourceCommit",
                    input.target_commit_sha,
                )
                || !field_equals(&target.payload, "commitSha", input.target_commit_sha)
                || !field_equals(
                    &compensation.payload,
                    "commitSha",
                    input.compensation_commit_sha,
                )
                || !field_equals(&target.payload, "family", input.family.wire())
                || !field_equals(&compensation.payload, "family", input.family.wire())
                || !field_equals(&target.payload, "fencingToken", input.fencing_token)
                || !field_equals(&compensation.payload, "fencingToken", input.fencing_token)
                || !field_equals(&lease.payload, "fencingToken", input.fencing_token)
                || !field_equals(&lease.payload, "dueKey", &target_due)
                || !field_equals(&lease.payload, "runId", &target_run)
                || !field_equals(&compensation.payload, "releaseDueKey", &target_due)
                || !field_equals(&compensation.payload, "releaseRunId", &target_run)
                || final_model_release_tag(records, target).as_deref()
                    != Some(input.target_release_tag)
                || final_model_release_tag(records, compensation).as_deref()
                    != Some(input.compensation_release_tag)
            {
                return Err(Error::ModelReleasePairUnbound);
            }
            let due_key = target.due_key.as_deref().ok_or(Error::State)?;
            unique_addition(
                records,
                addition(
                    due_key,
                    &receipt.kind,
                    receipt.record_key.clone(),
                    input.now,
                    payload.clone(),
                ),
            )
        },
    )?;
    Ok(receipt)
}

// validate one release-scope acquisition before observing journal state
fn validate_release_lease_input(
    input: NativeMaintenanceReleaseLeaseInput<'_>,
) -> Result<(), NativeMaintenanceModelReleaseJournalError> {
    use NativeMaintenanceModelReleaseJournalError::Input;
    // close every source operand before taking the durable journal lock
    if !valid_due_key(input.due_key)
        || !valid_hash(input.input_head_sha256)
        || !valid_run_id(input.run_id)
        || instant(input.now).is_err()
    {
        return Err(Input);
    }
    Ok(())
}

// validate one target preparation using the source's closed grammars
fn validate_model_release_input(
    input: NativeMaintenanceModelReleasePrepareInput<'_>,
) -> Result<(), NativeMaintenanceModelReleaseJournalError> {
    use NativeMaintenanceModelReleaseJournalError::Input;
    // require all immutable action, Git, release, lease, and clock identities
    if !valid_hash(input.action_sha256)
        || !valid_release_ref(input.action_tag)
        || !valid_release_ref(input.branch)
        || !valid_git_commit(input.commit_sha)
        || !valid_git_commit(input.expected_source_commit)
        || !valid_fencing_token(input.fencing_token)
        || instant(input.now).is_err()
        || !valid_hash(input.package_root_sha256)
        || !valid_due_key(input.release_due_key)
        || !valid_run_id(input.release_run_id)
        || !valid_release_tag(input.release_tag)
    {
        return Err(Input);
    }
    Ok(())
}

// validate one compensation preparation using the source's closed grammars
fn validate_compensating_release_input(
    input: NativeMaintenanceCompensatingModelReleasePrepareInput<'_>,
) -> Result<(), NativeMaintenanceModelReleaseJournalError> {
    use NativeMaintenanceModelReleaseJournalError::Input;
    // validate the common model-release fields and the additional predecessor identity
    if !valid_hash(input.action_sha256)
        || !valid_hash(input.predecessor_action_sha256)
        || !valid_release_ref(input.action_tag)
        || !valid_release_ref(input.branch)
        || !valid_git_commit(input.commit_sha)
        || !valid_git_commit(input.expected_source_commit)
        || !valid_fencing_token(input.fencing_token)
        || instant(input.now).is_err()
        || !valid_hash(input.package_root_sha256)
        || !valid_due_key(input.release_due_key)
        || !valid_run_id(input.release_run_id)
        || !valid_release_tag(input.release_tag)
    {
        return Err(Input);
    }
    Ok(())
}

// validate one collision record without granting Git publication authority
fn validate_collision_input(
    input: NativeMaintenanceModelReleaseTagCollisionInput<'_>,
) -> Result<(), NativeMaintenanceModelReleaseJournalError> {
    use NativeMaintenanceModelReleaseJournalError::Input;
    // admit only the exact prepared action, conflicting commit, tags, fence, and clock
    if !valid_hash(input.action_sha256)
        || !valid_release_tag(input.attempted_release_tag)
        || !valid_git_commit(input.collision_commit_sha)
        || !valid_fencing_token(input.fencing_token)
        || instant(input.now).is_err()
        || !valid_release_tag(input.successor_release_tag)
    {
        return Err(Input);
    }
    Ok(())
}

// validate one published pair receipt without observing remote refs
fn validate_pair_input(
    input: NativeMaintenanceModelReleasePairInput<'_>,
) -> Result<(), NativeMaintenanceModelReleaseJournalError> {
    use NativeMaintenanceModelReleaseJournalError::Input;
    // close every target and compensation identity before taking the journal lock
    if !valid_hash(input.compensation_action_sha256)
        || !valid_git_commit(input.compensation_commit_sha)
        || !valid_release_tag(input.compensation_release_tag)
        || !valid_fencing_token(input.fencing_token)
        || instant(input.now).is_err()
        || !valid_hash(input.target_action_sha256)
        || !valid_git_commit(input.target_commit_sha)
        || !valid_release_tag(input.target_release_tag)
    {
        return Err(Input);
    }
    Ok(())
}

// construct the exact acquired release payload
fn release_lease_payload(lease: &NativeMaintenanceReleaseLease) -> JsonValue {
    object(vec![
        ("scope", string(RELEASE_SCOPE)),
        ("runId", string(&lease.run_id)),
        ("dueKey", string(&lease.due_key)),
        ("acquiredAt", string(&lease.acquired_at)),
        ("expiresAt", string(&lease.expires_at)),
        ("inputHeadSha256", string(&lease.input_head_sha256)),
        ("fencingToken", string(&lease.fencing_token)),
    ])
}

// decode one retained active release lease without normalizing its identities
fn release_lease(
    record: &NativeMaintenanceJournalRecord,
) -> Result<NativeMaintenanceReleaseLease, NativeMaintenanceModelReleaseJournalError> {
    use NativeMaintenanceModelReleaseJournalError::State;
    let lease = NativeMaintenanceReleaseLease {
        acquired_at: text(&record.payload, "acquiredAt").ok_or(State)?,
        due_key: text(&record.payload, "dueKey").ok_or(State)?,
        expires_at: text(&record.payload, "expiresAt").ok_or(State)?,
        fencing_token: text(&record.payload, "fencingToken").ok_or(State)?,
        input_head_sha256: text(&record.payload, "inputHeadSha256").ok_or(State)?,
        run_id: text(&record.payload, "runId").ok_or(State)?,
    };
    // retain only exact source release-scope acquisition material
    if text(&record.payload, "scope").as_deref() != Some(RELEASE_SCOPE)
        || !valid_due_key(&lease.due_key)
        || !valid_hash(&lease.input_head_sha256)
        || !valid_run_id(&lease.run_id)
        || !valid_fencing_token(&lease.fencing_token)
        || instant(&lease.acquired_at).is_err()
        || instant(&lease.expires_at).is_err()
    {
        return Err(State);
    }
    Ok(lease)
}

// allocate one greater fence from all durable release acquisitions
fn allocate_fencing_token(
    records: &[NativeMaintenanceJournalRecord],
) -> Result<u64, NativeMaintenanceModelReleaseJournalError> {
    use NativeMaintenanceModelReleaseJournalError::{FencingTokenExhausted, State};
    let mut maximum = 0_u64;
    // retain only non-null lease-acquisition fences and fail closed on malformed history
    for record in records
        .iter()
        .filter(|record| kind(record, "lease_acquired"))
    {
        // ordinary lease scopes retain an explicit null fence
        if matches!(
            field(&record.payload, "fencingToken"),
            Some(JsonValue::Null)
        ) {
            continue;
        }
        let fencing_token = text(&record.payload, "fencingToken")
            .filter(|value| valid_fencing_token(value))
            .and_then(|value| value.parse::<u64>().ok())
            .ok_or(State)?;
        maximum = maximum.max(fencing_token);
    }
    maximum.checked_add(1).ok_or(FencingTokenExhausted)
}

// require one exact current global release lease
fn require_release_lease(
    record: Option<&&NativeMaintenanceJournalRecord>,
    due_key: &str,
    run_id: &str,
    fencing_token: &str,
) -> Result<(), NativeMaintenanceModelReleaseJournalError> {
    use NativeMaintenanceModelReleaseJournalError::ReleaseLeaseUnavailable;
    // bind the active record to all three external release coordinates
    if record.is_none_or(|record| {
        !field_equals(&record.payload, "dueKey", due_key)
            || !field_equals(&record.payload, "runId", run_id)
            || !field_equals(&record.payload, "fencingToken", fencing_token)
    }) {
        return Err(ReleaseLeaseUnavailable);
    }
    Ok(())
}

// detect one prepared confirmation action without its durable terminal reconciliation
fn has_unreconciled_prepared_action(
    records: &[NativeMaintenanceJournalRecord],
    lease: &NativeMaintenanceReleaseLease,
) -> Result<bool, NativeMaintenanceModelReleaseJournalError> {
    use NativeMaintenanceModelReleaseJournalError::State;
    let prepared = records.iter().find(|record| {
        kind(record, "confirmation_action_prepared")
            && field_equals(&record.payload, "releaseRunId", &lease.run_id)
            && field_equals(&record.payload, "releaseDueKey", &lease.due_key)
            && field_equals(&record.payload, "fencingToken", &lease.fencing_token)
    });
    let Some(prepared) = prepared else {
        return Ok(false);
    };
    let registration = text(&prepared.payload, "registrationSha256").ok_or(State)?;
    let references = records.iter().collect::<Vec<_>>();
    Ok(!is_terminal(&references, &registration))
}

// construct the exact target preparation payload
fn model_release_payload(input: NativeMaintenanceModelReleasePrepareInput<'_>) -> JsonValue {
    object(vec![
        ("actionSha256", string(input.action_sha256)),
        ("actionTag", string(input.action_tag)),
        ("branch", string(input.branch)),
        ("commitSha", string(input.commit_sha)),
        ("expectedSourceCommit", string(input.expected_source_commit)),
        ("family", string(input.family.wire())),
        ("fencingToken", string(input.fencing_token)),
        ("packageRootSha256", string(input.package_root_sha256)),
        ("releaseDueKey", string(input.release_due_key)),
        ("releaseRunId", string(input.release_run_id)),
        ("releaseTag", string(input.release_tag)),
    ])
}

// construct the exact compensation preparation payload
fn compensating_release_payload(
    input: NativeMaintenanceCompensatingModelReleasePrepareInput<'_>,
) -> JsonValue {
    object(vec![
        ("actionSha256", string(input.action_sha256)),
        ("actionTag", string(input.action_tag)),
        ("branch", string(input.branch)),
        ("commitSha", string(input.commit_sha)),
        ("expectedSourceCommit", string(input.expected_source_commit)),
        ("family", string(input.family.wire())),
        ("fencingToken", string(input.fencing_token)),
        ("packageRootSha256", string(input.package_root_sha256)),
        (
            "predecessorActionSha256",
            string(input.predecessor_action_sha256),
        ),
        ("releaseDueKey", string(input.release_due_key)),
        ("releaseRunId", string(input.release_run_id)),
        ("releaseTag", string(input.release_tag)),
    ])
}

// append only a genuinely absent logical record or accept its exact immutable retry
fn unique_addition(
    records: &[NativeMaintenanceJournalRecord],
    addition: NativeMaintenanceOwnedJournalAddition,
) -> Result<Vec<NativeMaintenanceOwnedJournalAddition>, NativeMaintenanceModelReleaseJournalError> {
    use NativeMaintenanceModelReleaseJournalError::{JournalRecordCollision, State};
    let Some(existing) = records
        .iter()
        .find(|record| record.record_key == addition.record_key)
    else {
        return Ok(vec![addition]);
    };
    let payload_bytes = NativeWirePrimitives
        .encode_sorted_json(&addition.payload)
        .ok_or(State)?;
    let payload_sha256 = format_digest(RevisionArchiveCanonicalCodec::sha256(
        &NativeWirePrimitives,
        &payload_bytes,
    ));
    // preserve source idempotence only for the exact kind, due coordinate, and payload identity
    if !kind(existing, &addition.kind)
        || existing.due_key != addition.due_key
        || existing.payload_sha256 != payload_sha256
    {
        return Err(JournalRecordCollision);
    }
    Ok(Vec::new())
}

// find one immutable target or compensation preparation by action identity
fn find_model_release_preparation<'a>(
    records: &'a [NativeMaintenanceJournalRecord],
    action_sha256: &str,
) -> Option<&'a NativeMaintenanceJournalRecord> {
    records.iter().find(|record| {
        is_model_release_preparation(record)
            && field_equals(&record.payload, "actionSha256", action_sha256)
    })
}

// recognize either exact preparation event kind
fn is_model_release_preparation(record: &NativeMaintenanceJournalRecord) -> bool {
    kind(record, "model_release_prepared") || kind(record, "model_compensating_release_prepared")
}

// derive the last collision-adjusted release tag in durable journal order
fn final_model_release_tag(
    records: &[NativeMaintenanceJournalRecord],
    prepared: &NativeMaintenanceJournalRecord,
) -> Option<String> {
    let action_sha256 = text(&prepared.payload, "actionSha256")?;
    records
        .iter()
        .rev()
        .find(|record| {
            kind(record, "model_release_tag_collision")
                && field_equals(&record.payload, "actionSha256", &action_sha256)
        })
        .and_then(|record| text(&record.payload, "successorReleaseTag"))
        .or_else(|| text(&prepared.payload, "releaseTag"))
}

// compare one exact retained string member
fn field_equals(value: &JsonValue, key: &str, expected: &str) -> bool {
    text(value, key).as_deref() == Some(expected)
}

// compare one retained journal event discriminator
fn kind(record: &NativeMaintenanceJournalRecord, expected: &str) -> bool {
    matches!(&record.kind, JsonValue::String(units) if units.iter().copied().eq(expected.encode_utf16()))
}

// parse one exact lowercase SHA-256 string
fn parse_hash(value: &str) -> Option<[u8; 32]> {
    let digest = weather_edge::RevisionGapSha256::from_lower_hex(value)?;
    Some(*digest.as_bytes())
}

// admit one canonical source UTC millisecond instant
fn instant(value: &str) -> Result<i64, NativeMaintenanceModelReleaseJournalError> {
    NativeWirePrimitives
        .canonical_utc_milliseconds(value)
        .ok_or(NativeMaintenanceModelReleaseJournalError::Input)
}

// recognize one exact lowercase Git commit identity
fn valid_git_commit(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

// recognize one canonical positive unsigned 64-bit fence
fn valid_fencing_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 20
        && matches!(value.as_bytes().first(), Some(b'1'..=b'9'))
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && value.parse::<u64>().is_ok()
}

// recognize one bounded source due key without traversal-like segments
fn valid_due_key(value: &str) -> bool {
    let Some((class, suffix)) = value.split_once('/') else {
        return false;
    };
    value.len() <= 256
        && matches!(
            class,
            "archive"
                | "capture"
                | "daily"
                | "monthly"
                | "control-reference"
                | "confirmation"
                | "release"
                | "rollback"
        )
        && !suffix.is_empty()
        && suffix.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b':' | b'.' | b'_' | b'/' | b'-')
        })
        && !value.contains("..")
        && !value.contains("//")
}

// recognize one bounded source run identity
fn valid_run_id(value: &str) -> bool {
    (1..=128).contains(&value.len())
        && value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && value
            .bytes()
            .skip(1)
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

// recognize one closed automation ref suffix
fn valid_release_ref(value: &str) -> bool {
    (1..=200).contains(&value.len())
        && value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && value
            .bytes()
            .skip(1)
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'/' | b'-'))
        && !value.contains("..")
        && !value.contains("//")
        && !value.ends_with('/')
        && !value.ends_with('.')
}

// recognize one immutable calendar release tag
fn valid_release_tag(value: &str) -> bool {
    let bytes = value.as_bytes();
    let revision = bytes.get(11..).unwrap_or_default();
    bytes.len() >= 12
        && bytes.get(4) == Some(&b'.')
        && bytes.get(7) == Some(&b'.')
        && bytes.get(10) == Some(&b'-')
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[8..10].iter().all(u8::is_ascii_digit)
        && matches!(revision.first(), Some(b'1'..=b'9'))
        && revision.iter().all(u8::is_ascii_digit)
}

#[cfg(test)]
#[path = "native_maintenance_model_release_journal_test.rs"]
mod tests;
