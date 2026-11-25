//! Execute one fenced family release through explicit durable and privileged host effects.

use std::fmt::{self, Display, Formatter};

use weather_contracts::JsonValue;
use weather_edge::ForecastAdjustmentSettingsJsonAdapter;

use crate::maintenance_canonical_json::{MaintenanceCanonicalJson, field, text, valid_hash};
use crate::{NativeEdgeAdminJson, NativeWirePrimitives};

const MODEL_ACTION_VERSION: &str = "forecast-adjustment-model-action/v1";
const CONTROL_ACTION_VERSION: &str = "forecast-adjustment-rain-control-reference-action/v1";
const DEVELOPMENT_AUTHORITY_VERSION: &str = "adjustment-development-custody-anchor/v1";
const CONTROL_AUTHORITY_VERSION: &str = "adjustment-rain-control-custody-anchor/v1";
const FUTURE_AUTHORITY_VERSION: &str = "adjustment-maintenance-anchor/v3";

/// The predecessor's closed nine operands after transport decoding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeFamilyReleaseInput {
    pub action_sha256: String,
    pub compensating_release: String,
    pub expected_current_release: String,
    pub expected_settings_sha256: String,
    pub expected_source_release: String,
    pub family: String,
    pub fencing_token: String,
    pub report_sha256: String,
    pub target_release: String,
}

/// Current source, settings, fence, and installed-slot identities observed under the release lock.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeFamilyReleaseCurrentObservation {
    pub commit: String,
    pub release: String,
    pub settings_bytes: Vec<u8>,
    pub maximum_fence: String,
    pub active_installed_receipt_sha256: Option<String>,
    pub shadow_installed_receipt_sha256: Option<String>,
    pub control_installed_receipt_sha256: Option<String>,
}

/// Immutable proof material that the persistence owner must retain before target mutation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeFamilyReleaseDurableProof {
    pub baseline_catalog_bytes: Vec<u8>,
    pub compensation_action_sha256: String,
    pub compensation_commit: String,
    pub compensation_family_identity_sha256: String,
    pub compensation_image_digests_sha256: String,
    pub source_settings_bytes: Vec<u8>,
    pub target_catalog_bytes: Vec<u8>,
    pub target_commit: String,
    pub target_family_identity_sha256: String,
    pub target_image_digests_sha256: String,
}

/// Authority and actions admitted by the separately privileged source/image proof owner.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeFamilyReleaseVerifiedProof {
    pub action: JsonValue,
    pub authority: JsonValue,
    pub compensation_action: JsonValue,
    pub durable: NativeFamilyReleaseDurableProof,
}

/// The four durable mutation states retained by the source transaction document.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeFamilyReleaseMutationState {
    Prepared,
    Applying,
    CompensationRequired,
    Acknowledged,
}

impl NativeFamilyReleaseMutationState {
    /// Return the exact predecessor state spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Prepared => "prepared",
            Self::Applying => "applying",
            Self::CompensationRequired => "compensation_required",
            Self::Acknowledged => "acknowledged",
        }
    }
}

/// The two acknowledged target outcomes after restrictive settings reconciliation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeFamilyReleaseOutcome {
    Active,
    DeployedOperatorOff,
}

impl NativeFamilyReleaseOutcome {
    /// Return the exact predecessor outcome spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::DeployedOperatorOff => "deployed_operator_off",
        }
    }
}

/// One durable target mutation record, excluding the separately retained proof and compensation status.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeFamilyReleaseMutationRecord {
    pub action: JsonValue,
    pub created_at: String,
    pub failed_at: Option<String>,
    pub input: NativeFamilyReleaseInput,
    pub outcome: Option<NativeFamilyReleaseOutcome>,
    pub source_commit: String,
    pub source_settings_sha256: String,
    pub state: NativeFamilyReleaseMutationState,
    pub target_commit: String,
    pub verified_at: Option<String>,
}

/// One durable no-mutation acknowledgement when the selected family is operator-disabled.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeFamilyReleaseOperatorOffRecord {
    pub completed_at: String,
    pub input: NativeFamilyReleaseInput,
    pub source_commit: String,
    pub source_settings_sha256: String,
}

/// Either original durable transaction document shape returned by replay or execution.
#[derive(Clone, Debug, PartialEq)]
pub enum NativeFamilyReleasePriorRecord {
    Mutation(NativeFamilyReleaseMutationRecord),
    OperatorOff(NativeFamilyReleaseOperatorOffRecord),
}

/// Independently verified live state after the target release has been applied.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeFamilyReleaseLiveObservation {
    pub commit: String,
    pub family: String,
    pub family_identity_sha256: String,
    pub image_digests_sha256: String,
    pub release: String,
    pub settings_bytes: Vec<u8>,
}

/// Exact transaction result, preserving the source's disjoint operator-off document.
#[derive(Clone, Debug, PartialEq)]
pub enum NativeFamilyReleaseTransactionResult {
    Mutation(NativeFamilyReleaseMutationRecord),
    OperatorOff(NativeFamilyReleaseOperatorOffRecord),
}

/// Stable transaction refusal categories without leaking privileged host diagnostics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeFamilyReleaseTransactionError {
    Input,
    Lock,
    IdentityCollision,
    SourceCompareAndSwap,
    OperatorOffPersistence,
    Proof,
    PreparePersistence,
    ApplyingPersistence,
    Apply,
    LiveVerification,
    FailurePersistence,
    AcknowledgementPersistence,
}

impl Display for NativeFamilyReleaseTransactionError {
    /// Render one bounded transaction failure category.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Input => "family transaction tokens are invalid",
            Self::Lock => "family release lock failed",
            Self::IdentityCollision => "family transaction identity collision",
            Self::SourceCompareAndSwap => {
                "family source, settings or fencing compare-and-swap failed"
            }
            Self::OperatorOffPersistence => {
                "family operator-off acknowledgement persistence failed"
            }
            Self::Proof => "family action proof differs",
            Self::PreparePersistence => "family transaction preparation failed",
            Self::ApplyingPersistence => "family applying transition failed",
            Self::Apply => "family target application failed",
            Self::LiveVerification => "family live deployment verification differs",
            Self::FailurePersistence => "family failure persistence failed",
            Self::AcknowledgementPersistence => "family acknowledgement persistence failed",
        })
    }
}

impl std::error::Error for NativeFamilyReleaseTransactionError {}

/// All durability and privileged host effects required by the original transaction state machine.
pub trait NativeFamilyReleaseTransactionPort {
    /// Hold the inherited kernel release lock for the complete callback.
    fn with_release_lock<T>(
        &mut self,
        operation: impl FnOnce(&mut Self) -> Result<T, NativeFamilyReleaseTransactionError>,
    ) -> Result<T, NativeFamilyReleaseTransactionError>;

    /// Return an actual canonical UTC clock observation.
    fn clock(&mut self) -> Result<String, NativeFamilyReleaseTransactionError>;

    /// Read the executor-owned durable record by immutable action identity.
    fn read_prepared(
        &mut self,
        action_sha256: &str,
    ) -> Result<Option<NativeFamilyReleasePriorRecord>, NativeFamilyReleaseTransactionError>;

    /// Observe source release, settings, installed slots, and the durable maximum fence.
    fn inspect_current(
        &mut self,
    ) -> Result<NativeFamilyReleaseCurrentObservation, NativeFamilyReleaseTransactionError>;

    /// Refresh an expired exact capacity observation after CAS and before the proof clock.
    fn prepare_proof(
        &mut self,
        _input: &NativeFamilyReleaseInput,
        _current: &NativeFamilyReleaseCurrentObservation,
    ) -> Result<(), NativeFamilyReleaseTransactionError> {
        Ok(())
    }

    /// Verify Git, image, capacity, custody, catalog, and compensation material without mutation.
    fn verify_proof(
        &mut self,
        input: &NativeFamilyReleaseInput,
        current: &NativeFamilyReleaseCurrentObservation,
        prior: Option<&NativeFamilyReleaseMutationRecord>,
        now: &str,
    ) -> Result<NativeFamilyReleaseVerifiedProof, NativeFamilyReleaseTransactionError>;

    /// Create or exactly replay one no-mutation record and advance its durable fence.
    fn acknowledge_operator_off(
        &mut self,
        record: &NativeFamilyReleaseOperatorOffRecord,
    ) -> Result<NativeFamilyReleaseOperatorOffRecord, NativeFamilyReleaseTransactionError>;

    /// Create or exactly replay the immutable prepared document and advance its durable fence.
    fn prepare(
        &mut self,
        record: &NativeFamilyReleaseMutationRecord,
        proof: &NativeFamilyReleaseVerifiedProof,
    ) -> Result<(), NativeFamilyReleaseTransactionError>;

    /// Persist applying before the first catalog or service mutation.
    fn mark_applying(
        &mut self,
        record: &NativeFamilyReleaseMutationRecord,
    ) -> Result<(), NativeFamilyReleaseTransactionError>;

    /// Apply only the preverified target, compensating internally on target failure.
    fn apply_images(
        &mut self,
        record: &NativeFamilyReleaseMutationRecord,
        proof: &NativeFamilyReleaseVerifiedProof,
    ) -> Result<(), NativeFamilyReleaseTransactionError>;

    /// Verify the resulting live release, compensating internally on verification failure.
    fn verify_live(
        &mut self,
        record: &NativeFamilyReleaseMutationRecord,
        proof: &NativeFamilyReleaseVerifiedProof,
    ) -> Result<NativeFamilyReleaseLiveObservation, NativeFamilyReleaseTransactionError>;

    /// Persist compensation-required without erasing any already verified compensation result.
    fn record_failure(
        &mut self,
        record: &NativeFamilyReleaseMutationRecord,
    ) -> Result<(), NativeFamilyReleaseTransactionError>;

    /// Publish future current authority and durably acknowledge the verified target.
    fn acknowledge(
        &mut self,
        record: &NativeFamilyReleaseMutationRecord,
        live: &NativeFamilyReleaseLiveObservation,
    ) -> Result<NativeFamilyReleaseMutationRecord, NativeFamilyReleaseTransactionError>;
}

/// Execute or replay one source-exact family transaction under the supplied real release lock.
pub fn execute_native_family_release_transaction(
    input: NativeFamilyReleaseInput,
    port: &mut impl NativeFamilyReleaseTransactionPort,
) -> Result<NativeFamilyReleaseTransactionResult, NativeFamilyReleaseTransactionError> {
    // reject every malformed operand before acquiring or reading privileged host state
    if !valid_input(&input) {
        return Err(NativeFamilyReleaseTransactionError::Input);
    }
    port.with_release_lock(|port| execute_locked(&input, port))
}

// execute the original state machine while the host owner retains the release lock
fn execute_locked(
    input: &NativeFamilyReleaseInput,
    port: &mut impl NativeFamilyReleaseTransactionPort,
) -> Result<NativeFamilyReleaseTransactionResult, NativeFamilyReleaseTransactionError> {
    let prior = port.read_prepared(&input.action_sha256)?;
    // retries retain exact input identity and short-circuit terminal or unsafe mutation states
    if let Some(record) = &prior {
        let prior_input = match record {
            NativeFamilyReleasePriorRecord::Mutation(record) => &record.input,
            NativeFamilyReleasePriorRecord::OperatorOff(record) => &record.input,
        };
        // refuse one content-address collision before any host observation
        if prior_input != input {
            return Err(NativeFamilyReleaseTransactionError::IdentityCollision);
        }
        // converge a response-lost operator-off acknowledgement through its durable owner
        if let NativeFamilyReleasePriorRecord::OperatorOff(record) = record {
            return port
                .acknowledge_operator_off(record)
                .map(NativeFamilyReleaseTransactionResult::OperatorOff);
        }
        let NativeFamilyReleasePriorRecord::Mutation(record) = record else {
            unreachable!();
        };
        // never repeat an acknowledged target mutation
        if record.state == NativeFamilyReleaseMutationState::Acknowledged {
            return Ok(NativeFamilyReleaseTransactionResult::Mutation(
                record.clone(),
            ));
        }
        // applying and failed records require the separate durable compensation operation
        if matches!(
            record.state,
            NativeFamilyReleaseMutationState::Applying
                | NativeFamilyReleaseMutationState::CompensationRequired
        ) {
            let mut replay = record.clone();
            replay.state = NativeFamilyReleaseMutationState::CompensationRequired;
            return Ok(NativeFamilyReleaseTransactionResult::Mutation(replay));
        }
    }

    let current = port.inspect_current()?;
    let before = decode_settings(
        &current.settings_bytes,
        NativeFamilyReleaseTransactionError::SourceCompareAndSwap,
    )?;
    let settings_sha256 = digest(&current.settings_bytes)?;
    let maximum_fence = parse_fence(&current.maximum_fence, true)
        .ok_or(NativeFamilyReleaseTransactionError::SourceCompareAndSwap)?;
    let fence = parse_fence(&input.fencing_token, false)
        .ok_or(NativeFamilyReleaseTransactionError::SourceCompareAndSwap)?;
    let prepared_fence_obsolete = matches!(
        prior.as_ref(),
        Some(NativeFamilyReleasePriorRecord::Mutation(record))
            if record.state == NativeFamilyReleaseMutationState::Prepared
                && fence < maximum_fence
    );
    // recheck the source, raw settings bytes, and monotonic fence under the actual release lock
    if current.release != input.expected_current_release
        || settings_sha256 != input.expected_settings_sha256
        || (prior.is_none() && fence <= maximum_fence)
        || prepared_fence_obsolete
        || !valid_commit(&current.commit)
    {
        return Err(NativeFamilyReleaseTransactionError::SourceCompareAndSwap);
    }

    // persist an exact no-mutation record when the operator has disabled this family
    if !family_enabled(&before, &input.family) {
        let record = NativeFamilyReleaseOperatorOffRecord {
            completed_at: port.clock()?,
            input: input.clone(),
            source_commit: current.commit,
            source_settings_sha256: input.expected_settings_sha256.clone(),
        };
        return port
            .acknowledge_operator_off(&record)
            .map(NativeFamilyReleaseTransactionResult::OperatorOff);
    }

    // refresh only after source CAS and the operator-enable check, never on durable replay
    port.prepare_proof(input, &current)?;
    let now = port.clock()?;
    let prior_mutation = prior.as_ref().and_then(|record| match record {
        NativeFamilyReleasePriorRecord::Mutation(record) => Some(record),
        NativeFamilyReleasePriorRecord::OperatorOff(_) => None,
    });
    let proof = port.verify_proof(input, &current, prior_mutation, &now)?;
    validate_proof(input, &current, &proof)?;
    let record = NativeFamilyReleaseMutationRecord {
        action: proof.action.clone(),
        created_at: prior_mutation.map_or_else(|| now.clone(), |record| record.created_at.clone()),
        failed_at: None,
        input: input.clone(),
        outcome: None,
        source_commit: current.commit,
        source_settings_sha256: input.expected_settings_sha256.clone(),
        state: NativeFamilyReleaseMutationState::Prepared,
        target_commit: proof.durable.target_commit.clone(),
        verified_at: None,
    };
    port.prepare(&record, &proof)
        .map_err(|_| NativeFamilyReleaseTransactionError::PreparePersistence)?;
    port.mark_applying(&record)
        .map_err(|_| NativeFamilyReleaseTransactionError::ApplyingPersistence)?;

    // every failure after applying is converted to a durable compensation-required record
    let result = apply_and_verify(input, &current.settings_bytes, &record, &proof, port);
    match result {
        Ok((live, outcome)) => {
            let mut acknowledged = record.clone();
            acknowledged.outcome = Some(outcome);
            acknowledged.state = NativeFamilyReleaseMutationState::Acknowledged;
            let acknowledgement = port
                .clock()
                .map_err(|_| NativeFamilyReleaseTransactionError::AcknowledgementPersistence)
                .and_then(|verified_at| {
                    acknowledged.verified_at = Some(verified_at);
                    port.acknowledge(&acknowledged, &live).map_err(|_| {
                        NativeFamilyReleaseTransactionError::AcknowledgementPersistence
                    })
                });
            // acknowledgement failures retain the same compensation-required recovery state
            match acknowledgement {
                Ok(record) => Ok(NativeFamilyReleaseTransactionResult::Mutation(record)),
                Err(error) => record_failed(record, port, error),
            }
        }
        Err(error) => record_failed(record, port, error),
    }
}

// retain the exact failed action and make a persistence failure supersede the triggering error
fn record_failed(
    mut record: NativeFamilyReleaseMutationRecord,
    port: &mut impl NativeFamilyReleaseTransactionPort,
    error: NativeFamilyReleaseTransactionError,
) -> Result<NativeFamilyReleaseTransactionResult, NativeFamilyReleaseTransactionError> {
    record.failed_at = Some(port.clock()?);
    record.state = NativeFamilyReleaseMutationState::CompensationRequired;
    port.record_failure(&record)
        .map_err(|_| NativeFamilyReleaseTransactionError::FailurePersistence)?;
    Err(error)
}

// apply the preverified target and independently validate its live identities and settings
fn apply_and_verify(
    input: &NativeFamilyReleaseInput,
    before_settings: &[u8],
    record: &NativeFamilyReleaseMutationRecord,
    proof: &NativeFamilyReleaseVerifiedProof,
    port: &mut impl NativeFamilyReleaseTransactionPort,
) -> Result<
    (
        NativeFamilyReleaseLiveObservation,
        NativeFamilyReleaseOutcome,
    ),
    NativeFamilyReleaseTransactionError,
> {
    port.apply_images(record, proof)
        .map_err(|_| NativeFamilyReleaseTransactionError::Apply)?;
    let live = port
        .verify_live(record, proof)
        .map_err(|_| NativeFamilyReleaseTransactionError::LiveVerification)?;
    let before = decode_settings(
        before_settings,
        NativeFamilyReleaseTransactionError::LiveVerification,
    )?;
    let after = decode_settings(
        &live.settings_bytes,
        NativeFamilyReleaseTransactionError::LiveVerification,
    )?;
    let outcome = reconcile_settings(&before, &after, &input.family)?;
    // bind health-admitted live state to the exact target proof and physical family identity
    if live.release != input.target_release
        || live.commit != proof.durable.target_commit
        || live.family != input.family
        || live.family_identity_sha256 != proof.durable.target_family_identity_sha256
        || live.image_digests_sha256 != proof.durable.target_image_digests_sha256
    {
        return Err(NativeFamilyReleaseTransactionError::LiveVerification);
    }
    Ok((live, outcome))
}

// recheck all cross-owner preservation constraints before the prepared document is written
fn validate_proof(
    input: &NativeFamilyReleaseInput,
    current: &NativeFamilyReleaseCurrentObservation,
    proof: &NativeFamilyReleaseVerifiedProof,
) -> Result<(), NativeFamilyReleaseTransactionError> {
    let durable = &proof.durable;
    let action_kind =
        text(&proof.action, "actionKind").ok_or(NativeFamilyReleaseTransactionError::Proof)?;
    let contract =
        text(&proof.action, "contractVersion").ok_or(NativeFamilyReleaseTransactionError::Proof)?;
    let control = contract == CONTROL_ACTION_VERSION;
    // require the verified owner's actions to retain their canonical content addresses
    if digest_json(&proof.action)? != input.action_sha256
        || digest_json(&proof.compensation_action)? != durable.compensation_action_sha256
        || !valid_hashes(durable)
        || durable.source_settings_bytes != current.settings_bytes
        || digest(&durable.source_settings_bytes)? != input.expected_settings_sha256
        || text(&proof.action, "family").as_deref() != Some(input.family.as_str())
        || text(&proof.action, "fencingToken").as_deref() != Some(input.fencing_token.as_str())
        || text(&proof.action, "expectedSettingsSha256").as_deref()
            != Some(input.expected_settings_sha256.as_str())
        || text(&proof.action, "expectedSourceCommit").as_deref() != Some(current.commit.as_str())
        || text(&proof.action, "expectedSourceRelease").as_deref() != Some(current.release.as_str())
        || !matches!(
            contract.as_str(),
            MODEL_ACTION_VERSION | CONTROL_ACTION_VERSION
        )
    {
        return Err(NativeFamilyReleaseTransactionError::Proof);
    }

    let installed = if control {
        current.control_installed_receipt_sha256.as_deref()
    } else if matches!(
        action_kind.as_str(),
        "shadow" | "promote" | "compensate_shadow"
    ) {
        current.shadow_installed_receipt_sha256.as_deref()
    } else {
        current.active_installed_receipt_sha256.as_deref()
    };
    let expected_installed = if control {
        optional_text(&proof.action, "expectedCatalogReceiptSha256")?
    } else {
        optional_text(&proof.action, "expectedInstalledReceiptSha256")?
    };
    // retain the exact installed-slot compare-and-swap and report binding from the action
    if expected_installed.as_deref() != installed
        || (control
            && text(&proof.action, "graphManifestSha256").as_deref()
                != Some(input.report_sha256.as_str()))
        || (!control
            && text(&proof.action, "policyReportSha256").as_deref()
                != Some(input.report_sha256.as_str()))
    {
        return Err(NativeFamilyReleaseTransactionError::Proof);
    }

    let compensation_kind = text(&proof.compensation_action, "actionKind")
        .ok_or(NativeFamilyReleaseTransactionError::Proof)?;
    // reserve only the prebuilt compensating release with inherited target evidence
    if !compensation_kind.starts_with("compensate_")
        || text(&proof.compensation_action, "predecessorActionSha256").as_deref()
            != Some(input.action_sha256.as_str())
        || text(&proof.compensation_action, "fencingToken") != text(&proof.action, "fencingToken")
        || field(&proof.compensation_action, "validThrough") != field(&proof.action, "validThrough")
        || text(&proof.compensation_action, "expectedSourceCommit").as_deref()
            != Some(durable.target_commit.as_str())
        || text(&proof.compensation_action, "expectedSourceRelease").as_deref()
            != Some(input.target_release.as_str())
    {
        return Err(NativeFamilyReleaseTransactionError::Proof);
    }
    // compare the action-family-specific evidence that compensation must preserve
    if control {
        require_equal_fields(
            &proof.action,
            &proof.compensation_action,
            &[
                "expectedCatalogReceiptSha256",
                "graphManifestSha256",
                "controlStateSha256",
                "ordinalArtifactSha256",
                "sourceMemberRootSha256",
                "sourceReceiptRootSha256",
                "dueMonth",
            ],
        )?;
    } else {
        require_equal_fields(
            &proof.action,
            &proof.compensation_action,
            &[
                "policyDecision",
                "fullMemberRootSha256",
                "reportCreatedAt",
                "expectedInstalledReceiptSha256",
            ],
        )?;
    }
    validate_authority(input, current, &proof.action, &proof.authority, control)
}

// bind the admitted action to the exact custody authority selected by its action domain
fn validate_authority(
    input: &NativeFamilyReleaseInput,
    current: &NativeFamilyReleaseCurrentObservation,
    action: &JsonValue,
    authority: &JsonValue,
    control: bool,
) -> Result<(), NativeFamilyReleaseTransactionError> {
    let version =
        text(authority, "contractVersion").ok_or(NativeFamilyReleaseTransactionError::Proof)?;
    let development = version == DEVELOPMENT_AUTHORITY_VERSION;
    let control_authority = version == CONTROL_AUTHORITY_VERSION;
    let report = if control_authority {
        text(authority, "graphManifestSha256")
    } else if development || version == FUTURE_AUTHORITY_VERSION {
        text(authority, "policyReportSha256")
    } else {
        text(authority, "reportSha256")
    };
    // preserve the shared action, report, source, and lifecycle authority bindings
    if control != control_authority
        || (development && control)
        || text(authority, "actionSha256").as_deref() != Some(input.action_sha256.as_str())
        || report.as_deref() != Some(input.report_sha256.as_str())
        || text(authority, "sourceCommit").as_deref() != Some(current.commit.as_str())
        || (!control_authority
            && field(authority, "lifecycleLedgerRootSha256")
                != field(action, "lifecycleHeadSha256"))
        || (!control_authority
            && !development
            && field(authority, "fullMemberRootSha256") != field(action, "fullMemberRootSha256"))
    {
        return Err(NativeFamilyReleaseTransactionError::Proof);
    }
    // retain the rain-control custody coordinates as a separate authority domain
    if control_authority
        && (!control
            || !equal_fields(
                authority,
                action,
                &[
                    "controlStateSha256",
                    "ordinalArtifactSha256",
                    "sourceMemberRootSha256",
                    "sourceReceiptRootSha256",
                    "fencingToken",
                    "dueMonth",
                ],
            ))
    {
        return Err(NativeFamilyReleaseTransactionError::Proof);
    }
    // retain inactive development authority only for its exact shadow candidate
    if development
        && (text(action, "actionKind").as_deref() != Some("shadow")
            || text(authority, "actionKind").as_deref() != Some("shadow")
            || !matches!(field(action, "fullMemberRootSha256"), Some(JsonValue::Null))
            || text(action, "policyDecision").as_deref() != Some("pending")
            || !equal_fields(
                authority,
                action,
                &["candidateGraphSha256", "candidateSha256", "family"],
            ))
    {
        return Err(NativeFamilyReleaseTransactionError::Proof);
    }
    // serving actions require finalized custody while shadow-only actions remain nonserving
    if !control_authority
        && !matches!(
            text(action, "actionKind").as_deref(),
            Some("shadow" | "compensate_shadow")
        )
        && text(authority, "ctfState").as_deref() != Some("finalized")
    {
        return Err(NativeFamilyReleaseTransactionError::Proof);
    }
    Ok(())
}

// validate the fixed input grammar without regex or dependency additions
fn valid_input(input: &NativeFamilyReleaseInput) -> bool {
    matches!(input.family.as_str(), "rain" | "temperature" | "wind")
        && [
            &input.action_sha256,
            &input.report_sha256,
            &input.expected_settings_sha256,
        ]
        .into_iter()
        .all(|value| valid_hash(value))
        && parse_fence(&input.fencing_token, false).is_some()
        && [
            &input.target_release,
            &input.compensating_release,
            &input.expected_current_release,
            &input.expected_source_release,
        ]
        .into_iter()
        .all(|value| valid_release(value))
        && input.target_release != input.compensating_release
        && input.target_release != input.expected_current_release
        && input.expected_current_release == input.expected_source_release
}

// recognize one immutable Weather release spelling from the original transaction grammar
fn valid_release(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 12
        && bytes.get(4) == Some(&b'.')
        && bytes.get(7) == Some(&b'.')
        && bytes.get(10) == Some(&b'-')
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[8..10].iter().all(u8::is_ascii_digit)
        && bytes[11].is_ascii_digit()
        && bytes[11] != b'0'
        && bytes[12..].iter().all(u8::is_ascii_digit)
}

// parse the source's decimal uint64 fence with separate zero admission
fn parse_fence(value: &str, allow_zero: bool) -> Option<u64> {
    // reject leading zeros and the empty string before the standard parser
    if value.is_empty()
        || !value.bytes().all(|byte| byte.is_ascii_digit())
        || (value.len() > 1 && value.starts_with('0'))
        || (!allow_zero && value == "0")
    {
        return None;
    }
    value.parse().ok()
}

// recognize the original lowercase Git commit spelling
fn valid_commit(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

// decode the actual four-field settings contract through the retained native owner
fn decode_settings(
    bytes: &[u8],
    error: NativeFamilyReleaseTransactionError,
) -> Result<weather_edge::ForecastAdjustmentSettings, NativeFamilyReleaseTransactionError> {
    ForecastAdjustmentSettingsJsonAdapter::decode(&mut NativeEdgeAdminJson, bytes)
        .map_err(|_| error)
}

// select the family flag from one admitted settings document
fn family_enabled(settings: &weather_edge::ForecastAdjustmentSettings, family: &str) -> bool {
    match family {
        "temperature" => settings.temperature,
        "wind" => settings.wind,
        "rain" => settings.rain,
        _ => false,
    }
}

// preserve every restrictive concurrent setting and return the selected family outcome
fn reconcile_settings(
    before: &weather_edge::ForecastAdjustmentSettings,
    after: &weather_edge::ForecastAdjustmentSettings,
    family: &str,
) -> Result<NativeFamilyReleaseOutcome, NativeFamilyReleaseTransactionError> {
    // prohibit any concurrent false-to-true operator change
    if (!before.temperature && after.temperature)
        || (!before.wind && after.wind)
        || (!before.rain && after.rain)
    {
        return Err(NativeFamilyReleaseTransactionError::LiveVerification);
    }
    Ok(if family_enabled(after, family) {
        NativeFamilyReleaseOutcome::Active
    } else {
        NativeFamilyReleaseOutcome::DeployedOperatorOff
    })
}

// hash arbitrary bytes with the approved native SHA-256 primitive
fn digest(bytes: &[u8]) -> Result<String, NativeFamilyReleaseTransactionError> {
    MaintenanceCanonicalJson::sha256(&mut NativeWirePrimitives, bytes)
        .map(|value| value.as_str().to_owned())
        .ok_or(NativeFamilyReleaseTransactionError::Proof)
}

// canonicalize and hash one admitted JSON action
fn digest_json(value: &JsonValue) -> Result<String, NativeFamilyReleaseTransactionError> {
    let bytes = MaintenanceCanonicalJson::encode_canonical_json(&mut NativeWirePrimitives, value)
        .ok_or(NativeFamilyReleaseTransactionError::Proof)?;
    digest(&bytes)
}

// admit one nullable string identity without treating absent and null as equivalent
fn optional_text(
    value: &JsonValue,
    key: &str,
) -> Result<Option<String>, NativeFamilyReleaseTransactionError> {
    match field(value, key) {
        Some(JsonValue::Null) => Ok(None),
        Some(JsonValue::String(units)) => String::from_utf16(units)
            .ok()
            .map(Some)
            .ok_or(NativeFamilyReleaseTransactionError::Proof),
        _ => Err(NativeFamilyReleaseTransactionError::Proof),
    }
}

// verify every durable content or physical identity before persistence
fn valid_hashes(proof: &NativeFamilyReleaseDurableProof) -> bool {
    [
        &proof.compensation_action_sha256,
        &proof.compensation_family_identity_sha256,
        &proof.compensation_image_digests_sha256,
        &proof.target_family_identity_sha256,
        &proof.target_image_digests_sha256,
    ]
    .into_iter()
    .all(|value| valid_hash(value))
        && valid_commit(&proof.target_commit)
        && valid_commit(&proof.compensation_commit)
        && !proof.baseline_catalog_bytes.is_empty()
        && !proof.target_catalog_bytes.is_empty()
}

// require a fixed family of source and compensation members to remain strictly equal
fn require_equal_fields(
    left: &JsonValue,
    right: &JsonValue,
    keys: &[&str],
) -> Result<(), NativeFamilyReleaseTransactionError> {
    // reject the first missing or changed preservation operand
    if !equal_fields(left, right, keys) {
        return Err(NativeFamilyReleaseTransactionError::Proof);
    }
    Ok(())
}

// compare one closed list of JSON scalar or null members
fn equal_fields(left: &JsonValue, right: &JsonValue, keys: &[&str]) -> bool {
    keys.iter().all(|key| {
        let left = field(left, key);
        left.is_some() && left == field(right, key)
    })
}
