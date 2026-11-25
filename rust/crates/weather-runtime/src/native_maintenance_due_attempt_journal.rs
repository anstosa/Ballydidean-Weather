//! Exact due-attempt and development-installation mutations over the verified journal store.

use std::fmt::{self, Display, Formatter};

use weather_contracts::JsonValue;
use weather_edge::RevisionArchiveCanonicalCodec;

use crate::maintenance_canonical_json::{exact_keys, field, text, valid_hash};
use crate::native_maintenance_archive_journal::{
    NativeMaintenanceArchiveJournalError, trusted_clock,
};
use crate::{
    MaintenanceCanonicalJson, NativeMaintenanceCaptureFamily, NativeMaintenanceJournalRecord,
    NativeMaintenanceJournalStore, NativeMaintenanceJournalStoreError,
    NativeMaintenanceOwnedJournalAddition, NativeWirePrimitives,
};

const DUE_REGISTERED_KIND: &str = "due_registered";
const DUE_ATTEMPT_CONTEXT_KIND: &str = "due_attempt_context";
const DUE_TERMINAL_OUTCOME_KIND: &str = "due_terminal_outcome";
const DUE_TERMINAL_RETIREMENT_KIND: &str = "due_terminal_retirement";
const DUE_TERMINAL_RETIREMENT_COMPLETED_KIND: &str = "due_terminal_retirement_completed";
const CONFIRMATION_REGISTERED_KIND: &str = "confirmation_registered";
const DEVELOPMENT_INSTALLED_KIND: &str = "confirmation_development_installed";

/// One immutable original-input and lifecycle-head pair for a due attempt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceDueAttemptContext {
    pub input_head_sha256: String,
    pub lifecycle_head_sha256: String,
}

/// The predecessor's closed daily terminal-outcome reason.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceDueTerminalReason {
    DailyCandidatePromoted,
    DailyCandidateRejected,
    DailySupportFailed,
}

impl NativeMaintenanceDueTerminalReason {
    /// Return the exact source reason spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DailyCandidatePromoted => "daily_candidate_promoted",
            Self::DailyCandidateRejected => "daily_candidate_rejected",
            Self::DailySupportFailed => "daily_support_failed",
        }
    }

    /// Admit one exact source reason spelling.
    fn parse(value: &str) -> Option<Self> {
        match value {
            "daily_candidate_promoted" => Some(Self::DailyCandidatePromoted),
            "daily_candidate_rejected" => Some(Self::DailyCandidateRejected),
            "daily_support_failed" => Some(Self::DailySupportFailed),
            _ => None,
        }
    }
}

/// One value-free completed daily outcome retained before owner retirement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceDueTerminalOutcome {
    pub action_eligible: bool,
    pub candidate_graph_sha256: Option<String>,
    pub candidate_sha256: Option<String>,
    pub fit_receipt_sha256: Option<String>,
    pub reason: NativeMaintenanceDueTerminalReason,
    pub semantic_input_sha256: String,
    pub serving_changed: bool,
    pub state: String,
}

impl NativeMaintenanceDueTerminalOutcome {
    /// Construct one source-valid completed outcome with no candidate-value exposure.
    pub fn completed(
        reason: NativeMaintenanceDueTerminalReason,
        semantic_input_sha256: String,
        serving_changed: bool,
    ) -> Option<Self> {
        let action_eligible = reason == NativeMaintenanceDueTerminalReason::DailyCandidatePromoted;
        let outcome = Self {
            action_eligible,
            candidate_graph_sha256: None,
            candidate_sha256: None,
            fit_receipt_sha256: None,
            reason,
            semantic_input_sha256,
            serving_changed,
            state: "completed".to_owned(),
        };
        outcome.valid().then_some(outcome)
    }

    /// Preserve every closed-field and cross-field rule from the source validator.
    fn valid(&self) -> bool {
        valid_hash(&self.semantic_input_sha256)
            && self.state == "completed"
            && self.candidate_graph_sha256.is_none()
            && self.candidate_sha256.is_none()
            && self.fit_receipt_sha256.is_none()
            && (self.reason == NativeMaintenanceDueTerminalReason::DailyCandidatePromoted)
                == self.action_eligible
            && (self.reason == NativeMaintenanceDueTerminalReason::DailyCandidatePromoted
                || !self.serving_changed)
    }

    /// Decode one exact retained terminal-outcome object.
    fn from_json(value: &JsonValue) -> Option<Self> {
        const KEYS: &[&str] = &[
            "actionEligible",
            "candidateGraphSha256",
            "candidateSha256",
            "fitReceiptSha256",
            "reason",
            "semanticInputSha256",
            "servingChanged",
            "state",
        ];
        // reject extensions and non-null candidate material
        if !exact_keys(value, KEYS)
            || !matches!(field(value, "candidateGraphSha256"), Some(JsonValue::Null))
            || !matches!(field(value, "candidateSha256"), Some(JsonValue::Null))
            || !matches!(field(value, "fitReceiptSha256"), Some(JsonValue::Null))
        {
            return None;
        }
        let outcome = Self {
            action_eligible: boolean(value, "actionEligible")?,
            candidate_graph_sha256: None,
            candidate_sha256: None,
            fit_receipt_sha256: None,
            reason: NativeMaintenanceDueTerminalReason::parse(&text(value, "reason")?)?,
            semantic_input_sha256: text(value, "semanticInputSha256")?,
            serving_changed: boolean(value, "servingChanged")?,
            state: text(value, "state")?,
        };
        outcome.valid().then_some(outcome)
    }

    /// Encode one exact source terminal-outcome object.
    fn to_json(&self) -> JsonValue {
        object(vec![
            ("actionEligible", JsonValue::Boolean(self.action_eligible)),
            ("candidateGraphSha256", JsonValue::Null),
            ("candidateSha256", JsonValue::Null),
            ("fitReceiptSha256", JsonValue::Null),
            ("reason", string(self.reason.as_str())),
            ("semanticInputSha256", string(&self.semantic_input_sha256)),
            ("servingChanged", JsonValue::Boolean(self.serving_changed)),
            ("state", string(&self.state)),
        ])
    }
}

/// One exact acknowledged inactive shadow archive binding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceDevelopmentCandidateInstalled {
    pub action_sha256: String,
    pub artifact_sha256: String,
    pub candidate_graph_sha256: String,
    pub candidate_sha256: String,
    pub family: NativeMaintenanceCaptureFamily,
    pub registration_sha256: String,
    pub shadow_registration_sha256: String,
}

/// The predecessor's closed owner-retirement command class.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceDueTerminalRetirementKind {
    QualifiedV3,
    UnsupportedV1,
}

impl NativeMaintenanceDueTerminalRetirementKind {
    /// Return the exact source command-class spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::QualifiedV3 => "qualified_v3",
            Self::UnsupportedV1 => "unsupported_v1",
        }
    }

    /// Admit one exact source command-class spelling.
    fn parse(value: &str) -> Option<Self> {
        match value {
            "qualified_v3" => Some(Self::QualifiedV3),
            "unsupported_v1" => Some(Self::UnsupportedV1),
            _ => None,
        }
    }
}

/// One immutable owner-retirement request with its durable completion state.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceDueTerminalRetirement {
    pub completed: bool,
    pub kind: NativeMaintenanceDueTerminalRetirementKind,
    pub request: JsonValue,
    pub request_sha256: String,
}

/// Borrowed input for freezing one original due-attempt context.
#[derive(Clone, Copy, Debug)]
pub struct NativeMaintenanceRecordDueAttemptContext<'a> {
    pub due_key: &'a str,
    pub input_head_sha256: &'a str,
    pub lifecycle_head_sha256: &'a str,
    pub now: &'a str,
}

/// Borrowed input for retaining one daily terminal outcome.
#[derive(Clone, Copy, Debug)]
pub struct NativeMaintenanceRecordDueTerminalOutcome<'a> {
    pub due_key: &'a str,
    pub now: &'a str,
    pub outcome: &'a NativeMaintenanceDueTerminalOutcome,
}

/// Borrowed input for retaining one acknowledged development installation.
#[derive(Clone, Copy, Debug)]
pub struct NativeMaintenanceRecordDevelopmentCandidateInstalled<'a> {
    pub action_sha256: &'a str,
    pub artifact_sha256: &'a str,
    pub candidate_graph_sha256: &'a str,
    pub candidate_sha256: &'a str,
    pub family: NativeMaintenanceCaptureFamily,
    pub now: &'a str,
    pub registration_sha256: &'a str,
    pub shadow_registration_sha256: &'a str,
}

/// Borrowed input for retaining one exact owner-retirement command.
#[derive(Clone, Copy, Debug)]
pub struct NativeMaintenanceRecordDueTerminalRetirement<'a> {
    pub due_key: &'a str,
    pub kind: NativeMaintenanceDueTerminalRetirementKind,
    pub now: &'a str,
    pub request: &'a JsonValue,
}

/// Borrowed input for acknowledging one exact owner-retirement command.
#[derive(Clone, Copy, Debug)]
pub struct NativeMaintenanceCompleteDueTerminalRetirement<'a> {
    pub due_key: &'a str,
    pub now: &'a str,
    pub request_sha256: &'a str,
}

/// Closed source refusals and durable-store failures for due-attempt mutations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceDueAttemptJournalError {
    Store(NativeMaintenanceJournalStoreError),
    Invalid,
    CorruptHistory,
    ClockUntrusted,
    ReconciliationRequired,
    DueAttemptContextUnbound,
    DueTerminalOutcomeUnregistered,
    DueTerminalRetirementUnregistered,
    DueTerminalRetirementUnbound,
    ConfirmationRegistrationUnknown,
    ConfirmationDevelopmentInstallationCollision,
    JournalRecordCollision,
}

impl Display for NativeMaintenanceDueAttemptJournalError {
    /// Render one path-free source failure category.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Store(_) => "maintenance due-attempt journal storage failed",
            Self::Invalid => "maintenance due-attempt journal input is invalid",
            Self::CorruptHistory => "maintenance due-attempt journal history is invalid",
            Self::ClockUntrusted => "clock_untrusted",
            Self::ReconciliationRequired => "reconciliation_required",
            Self::DueAttemptContextUnbound => "due_attempt_context_unbound",
            Self::DueTerminalOutcomeUnregistered => "due_terminal_outcome_unregistered",
            Self::DueTerminalRetirementUnregistered => "due_terminal_retirement_unregistered",
            Self::DueTerminalRetirementUnbound => "due_terminal_retirement_unbound",
            Self::ConfirmationRegistrationUnknown => "confirmation_registration_unknown",
            Self::ConfirmationDevelopmentInstallationCollision => {
                "confirmation_development_installation_collision"
            }
            Self::JournalRecordCollision => "journal_record_collision",
        })
    }
}

impl std::error::Error for NativeMaintenanceDueAttemptJournalError {
    /// Preserve only an actual durable-store source.
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        // semantic source refusals have no lower-level cause
        match self {
            Self::Store(error) => Some(error),
            _ => None,
        }
    }
}

impl From<NativeMaintenanceJournalStoreError> for NativeMaintenanceDueAttemptJournalError {
    /// Retain the actual native store failure.
    fn from(error: NativeMaintenanceJournalStoreError) -> Self {
        Self::Store(error)
    }
}

/// Concrete due-attempt journal owner over the one installed verified store.
pub struct NativeMaintenanceDueAttemptJournal<'a> {
    store: &'a mut NativeMaintenanceJournalStore,
}

impl<'a> NativeMaintenanceDueAttemptJournal<'a> {
    /// Borrow the actual journal initialized for the current controller.
    pub fn new(store: &'a mut NativeMaintenanceJournalStore) -> Self {
        Self { store }
    }

    /// Read one frozen original-input and lifecycle-head pair.
    pub fn read_due_attempt_context(
        &mut self,
        due_key: &str,
    ) -> Result<Option<NativeMaintenanceDueAttemptContext>, NativeMaintenanceDueAttemptJournalError>
    {
        use NativeMaintenanceDueAttemptJournalError as Error;
        // reject path escapes before reading journal state
        if !valid_due_key(due_key) {
            return Err(Error::Invalid);
        }
        let records = self
            .store
            .read_verified(&mut NativeWirePrimitives, &NativeWirePrimitives)?;
        records
            .iter()
            .find(|record| {
                is_kind(record, DUE_ATTEMPT_CONTEXT_KIND)
                    && record.due_key.as_deref() == Some(due_key)
            })
            .map(|record| parse_attempt_context(&record.payload).ok_or(Error::CorruptHistory))
            .transpose()
    }

    /// Freeze one original due-attempt context before irreversible work.
    pub fn record_due_attempt_context(
        &mut self,
        input: NativeMaintenanceRecordDueAttemptContext<'_>,
    ) -> Result<NativeMaintenanceDueAttemptContext, NativeMaintenanceDueAttemptJournalError> {
        use NativeMaintenanceDueAttemptJournalError as Error;
        // validate every caller-owned coordinate before taking the durable lock
        if !valid_due_key(input.due_key)
            || !valid_hash(input.input_head_sha256)
            || !valid_hash(input.lifecycle_head_sha256)
            || !valid_instant(input.now)
        {
            return Err(Error::Invalid);
        }
        let context = NativeMaintenanceDueAttemptContext {
            input_head_sha256: input.input_head_sha256.to_owned(),
            lifecycle_head_sha256: input.lifecycle_head_sha256.to_owned(),
        };
        let expected_payload = attempt_context_json(&context);
        let mut retained = None;
        self.store.mutate_checked(
            &mut NativeWirePrimitives,
            &NativeWirePrimitives,
            |records| {
                enforce_trusted_clock(records, input.now)?;
                let registration = records.iter().find(|record| {
                    is_kind(record, DUE_REGISTERED_KIND)
                        && record.due_key.as_deref() == Some(input.due_key)
                });
                // bind the frozen attempt to the registered semantic input
                if registration
                    .and_then(|record| text(&record.payload, "inputHeadSha256"))
                    .as_deref()
                    != Some(input.input_head_sha256)
                {
                    return Err(Error::DueAttemptContextUnbound);
                }
                let addition = unique_mutation(
                    records,
                    input.due_key,
                    DUE_ATTEMPT_CONTEXT_KIND,
                    format!("due/{}/attempt-context", input.due_key),
                    input.now,
                    expected_payload.clone(),
                )?;
                retained = Some(context.clone());
                Ok(addition.into_iter().collect())
            },
        )?;
        retained.ok_or(Error::CorruptHistory)
    }

    /// Read one terminal due outcome retained before owner retirement.
    pub fn read_due_terminal_outcome(
        &mut self,
        due_key: &str,
    ) -> Result<Option<NativeMaintenanceDueTerminalOutcome>, NativeMaintenanceDueAttemptJournalError>
    {
        use NativeMaintenanceDueAttemptJournalError as Error;
        // reject path escapes before reading journal state
        if !valid_due_key(due_key) {
            return Err(Error::Invalid);
        }
        let records = self
            .store
            .read_verified(&mut NativeWirePrimitives, &NativeWirePrimitives)?;
        records
            .iter()
            .find(|record| {
                is_kind(record, DUE_TERMINAL_OUTCOME_KIND)
                    && record.due_key.as_deref() == Some(due_key)
            })
            .map(|record| {
                field(&record.payload, "outcome")
                    .and_then(NativeMaintenanceDueTerminalOutcome::from_json)
                    .ok_or(Error::CorruptHistory)
            })
            .transpose()
    }

    /// Retain one exact terminal attempt outcome before owner retirement.
    pub fn record_due_terminal_outcome(
        &mut self,
        input: NativeMaintenanceRecordDueTerminalOutcome<'_>,
    ) -> Result<NativeMaintenanceDueTerminalOutcome, NativeMaintenanceDueAttemptJournalError> {
        use NativeMaintenanceDueAttemptJournalError as Error;
        // preserve the source validator before taking the durable lock
        if !valid_due_key(input.due_key) || !valid_instant(input.now) || !input.outcome.valid() {
            return Err(Error::Invalid);
        }
        let payload = object(vec![("outcome", input.outcome.to_json())]);
        let mut retained = None;
        self.store.mutate_checked(
            &mut NativeWirePrimitives,
            &NativeWirePrimitives,
            |records| {
                enforce_trusted_clock(records, input.now)?;
                // bind an outcome only to its already-registered original due
                if !records.iter().any(|record| {
                    is_kind(record, DUE_REGISTERED_KIND)
                        && record.due_key.as_deref() == Some(input.due_key)
                }) {
                    return Err(Error::DueTerminalOutcomeUnregistered);
                }
                let addition = unique_mutation(
                    records,
                    input.due_key,
                    DUE_TERMINAL_OUTCOME_KIND,
                    format!("due/{}/terminal-outcome", input.due_key),
                    input.now,
                    payload.clone(),
                )?;
                retained = Some(input.outcome.clone());
                Ok(addition.into_iter().collect())
            },
        )?;
        retained.ok_or(Error::CorruptHistory)
    }

    /// Read one retained owner-retirement request and its durable completion state.
    pub fn read_due_terminal_retirement(
        &mut self,
        due_key: &str,
    ) -> Result<
        Option<NativeMaintenanceDueTerminalRetirement>,
        NativeMaintenanceDueAttemptJournalError,
    > {
        use NativeMaintenanceDueAttemptJournalError as Error;
        // reject path escapes before reading journal state
        if !valid_due_key(due_key) {
            return Err(Error::Invalid);
        }
        let records = self
            .store
            .read_verified(&mut NativeWirePrimitives, &NativeWirePrimitives)?;
        let retained = records.iter().find(|record| {
            is_kind(record, DUE_TERMINAL_RETIREMENT_KIND)
                && record.due_key.as_deref() == Some(due_key)
        });
        let Some(retained) = retained else {
            return Ok(None);
        };
        let request_sha256 = text(&retained.payload, "requestSha256")
            .filter(|value| valid_hash(value))
            .ok_or(Error::CorruptHistory)?;
        let completed = records.iter().any(|record| {
            is_kind(record, DUE_TERMINAL_RETIREMENT_COMPLETED_KIND)
                && record.due_key.as_deref() == Some(due_key)
                && text(&record.payload, "requestSha256").as_deref()
                    == Some(request_sha256.as_str())
        });
        let kind = text(&retained.payload, "kind").ok_or(Error::CorruptHistory)?;
        let retirement = NativeMaintenanceDueTerminalRetirement {
            completed,
            kind: NativeMaintenanceDueTerminalRetirementKind::parse(&kind)
                .ok_or(Error::CorruptHistory)?,
            request: field(&retained.payload, "request")
                .filter(|value| matches!(value, JsonValue::Object(_)))
                .cloned()
                .ok_or(Error::CorruptHistory)?,
            request_sha256,
        };
        Ok(Some(retirement))
    }

    /// Retain one exact owner-retirement command before releasing the family slot.
    pub fn record_due_terminal_retirement(
        &mut self,
        input: NativeMaintenanceRecordDueTerminalRetirement<'_>,
    ) -> Result<NativeMaintenanceDueTerminalRetirement, NativeMaintenanceDueAttemptJournalError>
    {
        use NativeMaintenanceDueAttemptJournalError as Error;
        // admit only the predecessor's bounded command envelope
        if !valid_due_key(input.due_key)
            || !valid_instant(input.now)
            || !matches!(input.request, JsonValue::Object(_))
        {
            return Err(Error::Invalid);
        }
        let request_sha256 = payload_identity(input.request)?;
        let payload = object(vec![
            ("kind", string(input.kind.as_str())),
            ("request", input.request.clone()),
            ("requestSha256", string(&request_sha256)),
        ]);
        let mut retained = None;
        self.store.mutate_checked(
            &mut NativeWirePrimitives,
            &NativeWirePrimitives,
            |records| {
                enforce_trusted_clock(records, input.now)?;
                // bind the command to an already registered original due
                if !records.iter().any(|record| {
                    is_kind(record, DUE_REGISTERED_KIND)
                        && record.due_key.as_deref() == Some(input.due_key)
                }) {
                    return Err(Error::DueTerminalRetirementUnregistered);
                }
                let addition = unique_mutation(
                    records,
                    input.due_key,
                    DUE_TERMINAL_RETIREMENT_KIND,
                    format!("due/{}/terminal-retirement", input.due_key),
                    input.now,
                    payload.clone(),
                )?;
                let completed = records.iter().any(|record| {
                    is_kind(record, DUE_TERMINAL_RETIREMENT_COMPLETED_KIND)
                        && record.due_key.as_deref() == Some(input.due_key)
                        && text(&record.payload, "requestSha256").as_deref()
                            == Some(request_sha256.as_str())
                });
                retained = Some(NativeMaintenanceDueTerminalRetirement {
                    completed,
                    kind: input.kind,
                    request: input.request.clone(),
                    request_sha256: request_sha256.clone(),
                });
                Ok(addition.into_iter().collect())
            },
        )?;
        retained.ok_or(Error::CorruptHistory)
    }

    /// Acknowledge only the exact retained owner-retirement command.
    pub fn complete_due_terminal_retirement(
        &mut self,
        input: NativeMaintenanceCompleteDueTerminalRetirement<'_>,
    ) -> Result<(), NativeMaintenanceDueAttemptJournalError> {
        use NativeMaintenanceDueAttemptJournalError as Error;
        // reject changed or malformed command coordinates before locking
        if !valid_due_key(input.due_key)
            || !valid_instant(input.now)
            || !valid_hash(input.request_sha256)
        {
            return Err(Error::Invalid);
        }
        self.store.mutate_checked(
            &mut NativeWirePrimitives,
            &NativeWirePrimitives,
            |records| {
                enforce_trusted_clock(records, input.now)?;
                let retained = records.iter().find(|record| {
                    is_kind(record, DUE_TERMINAL_RETIREMENT_KIND)
                        && record.due_key.as_deref() == Some(input.due_key)
                });
                // reject naked or changed completion identities
                if retained
                    .and_then(|record| text(&record.payload, "requestSha256"))
                    .as_deref()
                    != Some(input.request_sha256)
                {
                    return Err(Error::DueTerminalRetirementUnbound);
                }
                let payload = object(vec![("requestSha256", string(input.request_sha256))]);
                let addition = unique_mutation(
                    records,
                    input.due_key,
                    DUE_TERMINAL_RETIREMENT_COMPLETED_KIND,
                    format!("due/{}/terminal-retirement-completed", input.due_key),
                    input.now,
                    payload,
                )?;
                Ok(addition.into_iter().collect())
            },
        )
    }

    /// Retain one acknowledged inactive shadow archive binding for cold replay.
    pub fn record_development_candidate_installed(
        &mut self,
        input: NativeMaintenanceRecordDevelopmentCandidateInstalled<'_>,
    ) -> Result<
        NativeMaintenanceDevelopmentCandidateInstalled,
        NativeMaintenanceDueAttemptJournalError,
    > {
        use NativeMaintenanceDueAttemptJournalError as Error;
        let hashes = [
            input.action_sha256,
            input.artifact_sha256,
            input.candidate_graph_sha256,
            input.candidate_sha256,
            input.registration_sha256,
            input.shadow_registration_sha256,
        ];
        // reject any noncanonical input before taking the durable lock
        if !valid_instant(input.now) || hashes.iter().any(|value| !valid_hash(value)) {
            return Err(Error::Invalid);
        }
        let installed = NativeMaintenanceDevelopmentCandidateInstalled {
            action_sha256: input.action_sha256.to_owned(),
            artifact_sha256: input.artifact_sha256.to_owned(),
            candidate_graph_sha256: input.candidate_graph_sha256.to_owned(),
            candidate_sha256: input.candidate_sha256.to_owned(),
            family: input.family,
            registration_sha256: input.registration_sha256.to_owned(),
            shadow_registration_sha256: input.shadow_registration_sha256.to_owned(),
        };
        let expected_payload = development_installed_json(&installed);
        let mut retained = None;
        self.store.mutate_checked(
            &mut NativeWirePrimitives,
            &NativeWirePrimitives,
            |records| {
                enforce_trusted_clock(records, input.now)?;
                let registration = records.iter().find(|record| {
                    is_kind(record, CONFIRMATION_REGISTERED_KIND)
                        && text(&record.payload, "family").as_deref() == Some(input.family.wire())
                        && text(&record.payload, "registrationSha256").as_deref()
                            == Some(input.registration_sha256)
                });
                // prohibit relabelling an acknowledged release onto another candidate
                if registration
                    .and_then(|record| text(&record.payload, "candidateSha256"))
                    .as_deref()
                    != Some(input.candidate_sha256)
                {
                    return Err(Error::ConfirmationRegistrationUnknown);
                }
                let registration = registration.ok_or(Error::ConfirmationRegistrationUnknown)?;
                let existing = records.iter().find(|record| {
                    is_kind(record, DEVELOPMENT_INSTALLED_KIND)
                        && text(&record.payload, "candidateSha256").as_deref()
                            == Some(input.candidate_sha256)
                });
                // reuse only byte-identical installed material
                if let Some(existing) = existing {
                    if payload_identity(&existing.payload)? != payload_identity(&expected_payload)?
                    {
                        return Err(Error::ConfirmationDevelopmentInstallationCollision);
                    }
                    retained = Some(
                        parse_development_installed(&existing.payload)
                            .ok_or(Error::CorruptHistory)?,
                    );
                    return Ok(Vec::new());
                }
                let due_key = registration
                    .due_key
                    .clone()
                    .filter(|value| valid_due_key(value))
                    .ok_or(Error::CorruptHistory)?;
                retained = Some(installed.clone());
                Ok(vec![NativeMaintenanceOwnedJournalAddition {
                    due_key: Some(due_key),
                    kind: DEVELOPMENT_INSTALLED_KIND.to_owned(),
                    payload: expected_payload.clone(),
                    record_key: format!(
                        "confirmation/{}/{}/development-installed",
                        input.family.wire(),
                        input.registration_sha256
                    ),
                    recorded_at: input.now.to_owned(),
                }])
            },
        )?;
        retained.ok_or(Error::CorruptHistory)
    }
}

/// Apply the exact clock rollback and expired-lease refusal categories.
fn enforce_trusted_clock(
    records: &[NativeMaintenanceJournalRecord],
    now: &str,
) -> Result<(), NativeMaintenanceDueAttemptJournalError> {
    use NativeMaintenanceArchiveJournalError as SourceError;
    use NativeMaintenanceDueAttemptJournalError as Error;
    trusted_clock(records, now, false)
        .map(|_| ())
        .map_err(|error| match error {
            SourceError::ClockUntrusted => Error::ClockUntrusted,
            SourceError::ReconciliationRequired => Error::ReconciliationRequired,
            _ => Error::CorruptHistory,
        })
}

/// Reuse one exact logical record key or return its single append event.
fn unique_mutation(
    records: &[NativeMaintenanceJournalRecord],
    due_key: &str,
    kind: &str,
    record_key: String,
    recorded_at: &str,
    payload: JsonValue,
) -> Result<Option<NativeMaintenanceOwnedJournalAddition>, NativeMaintenanceDueAttemptJournalError>
{
    use NativeMaintenanceDueAttemptJournalError as Error;
    let existing = records
        .iter()
        .find(|record| record.record_key == record_key);
    // preserve exact immutable idempotence independently of retry time
    if let Some(existing) = existing {
        if !is_kind(existing, kind)
            || existing.due_key.as_deref() != Some(due_key)
            || existing.payload_sha256 != payload_identity(&payload)?
        {
            return Err(Error::JournalRecordCollision);
        }
        return Ok(None);
    }
    Ok(Some(NativeMaintenanceOwnedJournalAddition {
        due_key: Some(due_key.to_owned()),
        kind: kind.to_owned(),
        payload,
        record_key,
        recorded_at: recorded_at.to_owned(),
    }))
}

/// Derive the canonical source payload identity without duplicating hashing.
fn payload_identity(
    payload: &JsonValue,
) -> Result<String, NativeMaintenanceDueAttemptJournalError> {
    let mut wire = NativeWirePrimitives;
    let bytes = wire
        .encode_canonical_json(payload)
        .ok_or(NativeMaintenanceDueAttemptJournalError::CorruptHistory)?;
    MaintenanceCanonicalJson::sha256(&mut wire, &bytes)
        .map(|digest| digest.as_str().to_owned())
        .ok_or(NativeMaintenanceDueAttemptJournalError::CorruptHistory)
}

/// Decode one exact due-attempt context payload.
fn parse_attempt_context(payload: &JsonValue) -> Option<NativeMaintenanceDueAttemptContext> {
    const KEYS: &[&str] = &["inputHeadSha256", "lifecycleHeadSha256"];
    let context = NativeMaintenanceDueAttemptContext {
        input_head_sha256: text(payload, "inputHeadSha256")?,
        lifecycle_head_sha256: text(payload, "lifecycleHeadSha256")?,
    };
    (exact_keys(payload, KEYS)
        && valid_hash(&context.input_head_sha256)
        && valid_hash(&context.lifecycle_head_sha256))
    .then_some(context)
}

/// Encode one exact due-attempt context payload.
fn attempt_context_json(context: &NativeMaintenanceDueAttemptContext) -> JsonValue {
    object(vec![
        ("inputHeadSha256", string(&context.input_head_sha256)),
        (
            "lifecycleHeadSha256",
            string(&context.lifecycle_head_sha256),
        ),
    ])
}

/// Decode one exact acknowledged development installation payload.
fn parse_development_installed(
    payload: &JsonValue,
) -> Option<NativeMaintenanceDevelopmentCandidateInstalled> {
    const KEYS: &[&str] = &[
        "actionSha256",
        "artifactSha256",
        "candidateGraphSha256",
        "candidateSha256",
        "family",
        "registrationSha256",
        "shadowRegistrationSha256",
    ];
    let family = match text(payload, "family")?.as_str() {
        "temperature" => NativeMaintenanceCaptureFamily::Temperature,
        "wind" => NativeMaintenanceCaptureFamily::Wind,
        "rain" => NativeMaintenanceCaptureFamily::Rain,
        _ => return None,
    };
    let installed = NativeMaintenanceDevelopmentCandidateInstalled {
        action_sha256: text(payload, "actionSha256")?,
        artifact_sha256: text(payload, "artifactSha256")?,
        candidate_graph_sha256: text(payload, "candidateGraphSha256")?,
        candidate_sha256: text(payload, "candidateSha256")?,
        family,
        registration_sha256: text(payload, "registrationSha256")?,
        shadow_registration_sha256: text(payload, "shadowRegistrationSha256")?,
    };
    let hashes = [
        installed.action_sha256.as_str(),
        installed.artifact_sha256.as_str(),
        installed.candidate_graph_sha256.as_str(),
        installed.candidate_sha256.as_str(),
        installed.registration_sha256.as_str(),
        installed.shadow_registration_sha256.as_str(),
    ];
    (exact_keys(payload, KEYS) && hashes.iter().all(|value| valid_hash(value))).then_some(installed)
}

/// Encode one exact acknowledged development installation payload.
fn development_installed_json(
    installed: &NativeMaintenanceDevelopmentCandidateInstalled,
) -> JsonValue {
    object(vec![
        ("actionSha256", string(&installed.action_sha256)),
        ("artifactSha256", string(&installed.artifact_sha256)),
        (
            "candidateGraphSha256",
            string(&installed.candidate_graph_sha256),
        ),
        ("candidateSha256", string(&installed.candidate_sha256)),
        ("family", string(installed.family.wire())),
        ("registrationSha256", string(&installed.registration_sha256)),
        (
            "shadowRegistrationSha256",
            string(&installed.shadow_registration_sha256),
        ),
    ])
}

/// Compare one verified journal event discriminator.
fn is_kind(record: &NativeMaintenanceJournalRecord, expected: &str) -> bool {
    matches!(&record.kind, JsonValue::String(units) if units.iter().copied().eq(expected.encode_utf16()))
}

/// Read one exact JSON boolean member.
fn boolean(value: &JsonValue, key: &str) -> Option<bool> {
    let JsonValue::Boolean(value) = field(value, key)? else {
        return None;
    };
    Some(*value)
}

/// Admit one canonical source instant without accepting aliases.
fn valid_instant(value: &str) -> bool {
    NativeWirePrimitives
        .canonical_utc_milliseconds(value)
        .is_some()
}

/// Recognize the predecessor's closed due-family and bounded suffix grammar.
fn valid_due_key(value: &str) -> bool {
    // split exactly once at the required family separator
    let Some((family, suffix)) = value.split_once('/') else {
        return false;
    };
    (1..=256).contains(&value.len())
        && matches!(
            family,
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
        && !value.contains("..")
        && !value.contains("//")
        && suffix.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b':' | b'.' | b'_' | b'/' | b'-')
        })
}

/// Build one canonical ordinary object in explicit source order.
fn object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(name, value)| (name.encode_utf16().collect(), value))
            .collect(),
    )
}

/// Build one source JSON string.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

#[cfg(test)]
#[path = "native_maintenance_due_attempt_journal_test.rs"]
mod tests;
