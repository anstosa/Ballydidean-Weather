use std::fmt::{self, Display, Formatter};
use std::future::Future;
use std::pin::Pin;

use weather_contracts::JsonValue;

use super::*;
use crate::{
    NativeBackendRuntime, NativeMaintenanceCaptureFamily, NativeMaintenanceDue,
    NativeMaintenanceDueMode, NativeWirePrimitives,
    native_maintenance_retirement_files_v2::format_digest,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FixtureError;

impl Display for FixtureError {
    /// Render the fixed fixture failure.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("fixture failure")
    }
}

impl std::error::Error for FixtureError {}

struct FixturePorts {
    evaluation: NativeMaintenanceDailyEvaluation,
    events: Vec<&'static str>,
    fit: NativeMaintenanceFitResult,
    persistence_available: bool,
}

impl NativeMaintenanceDueWorkPorts for FixturePorts {
    type Error = FixtureError;

    /// Admit the fixture control material without changing its bytes.
    fn validate_control_reference(
        &mut self,
        _due: &NativeMaintenanceDue,
        _now: &str,
        input: NativeMaintenanceControlReferenceInput,
    ) -> Result<NativeMaintenanceControlReferenceMaterial, Self::Error> {
        self.events.push("control");
        Ok(NativeMaintenanceControlReferenceMaterial { value: input.value })
    }

    /// Record unsupported daily completion order.
    fn complete_unsupported_daily<'a>(
        &'a mut self,
        _due: &'a NativeMaintenanceDue,
        _semantic_input_sha256: &'a str,
        _material: &'a NativeMaintenanceDailyUnsupportedMaterial,
    ) -> Pin<Box<dyn Future<Output = Result<(), Self::Error>> + 'a>> {
        Box::pin(async move {
            self.events.push("unsupported");
            Ok(())
        })
    }

    /// Return the selected closed daily fixture result.
    fn evaluate_daily(
        &mut self,
        _input: &NativeMaintenanceDailyEvaluationRequest,
    ) -> Result<NativeMaintenanceDailyEvaluation, Self::Error> {
        self.events.push("evaluate");
        Ok(self.evaluation.clone())
    }

    /// Record qualified completion and one real serving change.
    fn complete_qualified_daily<'a>(
        &'a mut self,
        _due: &'a NativeMaintenanceDue,
        _evaluation: &'a NativeMaintenanceDailyEvaluation,
        _material: &'a NativeMaintenanceDailyTerminalMaterial,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<NativeMaintenanceDailyQualifiedCompletion, Self::Error>>
                + 'a,
        >,
    > {
        Box::pin(async move {
            self.events.push("qualified");
            Ok(NativeMaintenanceDailyQualifiedCompletion {
                serving_changed: true,
            })
        })
    }

    /// Record one no-action completion.
    fn complete_no_action_daily<'a>(
        &'a mut self,
        _disposition: NativeMaintenanceDailyDisposition,
        _due: &'a NativeMaintenanceDue,
        _evaluation: &'a NativeMaintenanceDailyEvaluation,
        _material: &'a NativeMaintenanceDailyTerminalMaterial,
    ) -> Pin<Box<dyn Future<Output = Result<(), Self::Error>> + 'a>> {
        Box::pin(async move {
            self.events.push("no_action");
            Ok(())
        })
    }

    /// Return one source-shaped isolated fit result.
    fn run_fit<'a>(
        &'a mut self,
        _request: NativeMaintenanceFitRequest<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<NativeMaintenanceFitResult, Self::Error>> + 'a>> {
        Box::pin(async move {
            self.events.push("fit");
            Ok(self.fit.clone())
        })
    }

    /// Retain one installed parity result.
    fn build_candidate_parity(
        &mut self,
        _request: NativeMaintenanceCandidateParityRequest<'_>,
    ) -> Result<Option<NativeMaintenanceCandidateParity>, Self::Error> {
        self.events.push("parity");
        Ok(Some(NativeMaintenanceCandidateParity {
            value: JsonValue::Null,
        }))
    }

    /// Return the fixture persistence capability.
    fn candidate_persistence_available(&self) -> bool {
        self.persistence_available
    }

    /// Build complete confirmation and shadow registration material.
    fn build_candidate_registration<'a>(
        &'a mut self,
        _request: NativeMaintenanceCandidateRegistrationRequest<'a>,
    ) -> Pin<
        Box<
            dyn Future<
                    Output = Result<
                        Option<NativeMaintenanceCandidateRegistrationMaterial>,
                        Self::Error,
                    >,
                > + 'a,
        >,
    > {
        Box::pin(async move {
            self.events.push("registration");
            Ok(Some(NativeMaintenanceCandidateRegistrationMaterial {
                confirmation: JsonValue::Null,
                shadow_registration: Some(NativeMaintenanceShadowRegistrationMaterial {
                    artifact_sha256: hash_byte(b'a'),
                    registration_sha256: hash_byte(b'r'),
                }),
                value: JsonValue::Null,
            }))
        })
    }

    /// Persist the candidate before any local slot occupation.
    fn persist_candidate<'a>(
        &'a mut self,
        _request: NativeMaintenanceCandidatePersistenceRequest<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<NativeMaintenancePersistedCandidate, Self::Error>> + 'a>>
    {
        Box::pin(async move {
            self.events.push("persist");
            Ok(NativeMaintenancePersistedCandidate {
                candidate_graph_sha256: hash_byte(b'g'),
                value: JsonValue::Null,
            })
        })
    }

    /// Read the confirmation journal head after persistence.
    fn read_confirmation_head<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn Future<Output = Result<String, Self::Error>> + 'a>> {
        Box::pin(async move {
            self.events.push("head");
            Ok(hash_byte(b'h'))
        })
    }

    /// Record confirmation preregistration last.
    fn preregister_confirmation<'a>(
        &'a mut self,
        _confirmation: &'a JsonValue,
        _input_head_sha256: &'a str,
        _now: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<String, Self::Error>> + 'a>> {
        Box::pin(async move {
            self.events.push("preregister");
            Ok(hash_byte(b'p'))
        })
    }
}

/// Prove the daily promotion branch reports the exact action and terminal ordering.
#[test]
fn native_due_work_promotes_only_after_daily_evaluation() {
    let mut ports = fixture_ports(NativeMaintenanceDailyEvaluation::Promote {
        semantic_input_sha256: hash_byte(b's'),
        value: JsonValue::Null,
    });
    let outcome = NativeBackendRuntime::new()
        .expect("native runtime")
        .block_on(run_native_maintenance_due_work(
            &daily_due(),
            "2026-04-02T00:00:00.000Z",
            NativeMaintenanceSemanticInspection::DailyReady {
                input: NativeMaintenanceDailyEvaluationRequest {
                    value: JsonValue::Null,
                },
                input_manifest_sha256: hash_byte(b'i'),
                terminal_material: Some(NativeMaintenanceDailyTerminalMaterial {
                    value: JsonValue::Null,
                }),
            },
            &mut ports,
        ))
        .unwrap();
    assert_eq!(ports.events, ["evaluate", "qualified"]);
    assert_eq!(outcome.reason.as_str(), "daily_candidate_promoted");
    assert_eq!(outcome.state.as_str(), "completed");
    assert!(outcome.action_eligible);
    assert!(outcome.serving_changed);
    assert_eq!(outcome.semantic_input_sha256, Some(hash_byte(b's')));
}

/// Prove the monthly path preserves fit, parity, persistence, and slot ordering.
#[test]
fn native_due_work_archives_and_preregisters_monthly_candidate_in_source_order() {
    let mut ports = fixture_ports(NativeMaintenanceDailyEvaluation::Pending {
        semantic_input_sha256: hash_byte(b's'),
    });
    let outcome = NativeBackendRuntime::new()
        .expect("native runtime")
        .block_on(run_native_maintenance_due_work(
            &monthly_due(),
            "2026-04-01T00:00:00.000Z",
            monthly_inspection(),
            &mut ports,
        ))
        .unwrap();
    assert_eq!(
        ports.events,
        [
            "fit",
            "parity",
            "registration",
            "persist",
            "head",
            "preregister"
        ]
    );
    assert_eq!(outcome.reason.as_str(), "development_candidate_archived");
    assert_eq!(outcome.state.as_str(), "completed");
    assert_eq!(outcome.candidate_graph_sha256, Some(hash_byte(b'g')));
    assert_eq!(
        outcome.candidate_sha256,
        Some(hash_bytes(br#"{"family":"rain"}"#))
    );
    assert_eq!(
        outcome.fit_receipt_sha256.as_deref(),
        Some("9da68020c068d964458ba514a0bf5d65d8ae0fe58fbaded0cba8b4532c64d024")
    );
    let material = outcome.development_candidate.unwrap();
    assert_eq!(
        material.confirmation_registration_sha256,
        Some(hash_byte(b'p'))
    );
    assert_eq!(
        material
            .registration_material
            .unwrap()
            .shadow_registration
            .unwrap()
            .artifact_sha256,
        hash_byte(b'a')
    );
}

/// Prove unavailable persistence blocks after fitting and parity without side effects.
#[test]
fn native_due_work_blocks_before_registration_when_archive_is_unavailable() {
    let mut ports = fixture_ports(NativeMaintenanceDailyEvaluation::Pending {
        semantic_input_sha256: hash_byte(b's'),
    });
    ports.persistence_available = false;
    let outcome = NativeBackendRuntime::new()
        .expect("native runtime")
        .block_on(run_native_maintenance_due_work(
            &monthly_due(),
            "2026-04-01T00:00:00.000Z",
            monthly_inspection(),
            &mut ports,
        ))
        .unwrap();
    assert_eq!(ports.events, ["fit", "parity"]);
    assert_eq!(outcome.reason.as_str(), "candidate_archive_unavailable");
    assert_eq!(outcome.state.as_str(), "blocked");
    assert!(outcome.fit_receipt_sha256.is_some());
    assert!(outcome.development_candidate.is_none());
}

/// Build one complete effect fixture.
fn fixture_ports(evaluation: NativeMaintenanceDailyEvaluation) -> FixturePorts {
    FixturePorts {
        evaluation,
        events: Vec::new(),
        fit: fixture_fit(),
        persistence_available: true,
    }
}

/// Build the exact daily source projection consumed by the state machine.
fn daily_due() -> NativeMaintenanceDue {
    NativeMaintenanceDue {
        due_key: "daily/2026-04-01".to_owned(),
        family: None,
        mode: NativeMaintenanceDueMode::Daily,
        original_cutoff_at: "2026-04-02T07:00:00.000Z".to_owned(),
        scope: "daily".to_owned(),
    }
}

/// Build one free rain monthly due projection.
fn monthly_due() -> NativeMaintenanceDue {
    NativeMaintenanceDue {
        due_key: "monthly/rain/2026-04".to_owned(),
        family: Some(NativeMaintenanceCaptureFamily::Rain),
        mode: NativeMaintenanceDueMode::Monthly,
        original_cutoff_at: "2026-04-01T07:00:00.000Z".to_owned(),
        scope: "monthly/rain".to_owned(),
    }
}

/// Build one complete admitted monthly inspection.
fn monthly_inspection() -> NativeMaintenanceSemanticInspection {
    NativeMaintenanceSemanticInspection::MonthlyReady(NativeMaintenanceMonthlyReadyInspection {
        fit_input: NativeMaintenanceFitInput {
            value: JsonValue::Null,
        },
        input_manifest_sha256: hash_byte(b'i'),
        registration_context: Some(NativeMaintenanceRegistrationContext {
            value: JsonValue::Null,
        }),
        registration_slot: Some(NativeMaintenanceDueRegistrationSlot {
            family: NativeMaintenanceCaptureFamily::Rain,
            state: NativeMaintenanceDueRegistrationSlotState::Free,
        }),
        retained_parity_input: Some(NativeMaintenanceRetainedParityInput {
            value: JsonValue::Null,
        }),
        runtime_readiness: NativeMaintenanceAdmittedFitRuntimeReadiness {
            value: JsonValue::Null,
        },
        runtime_readiness_sha256: hash_byte(b'd'),
        synthetic_parity_input: Some(NativeMaintenanceSyntheticParityInput {
            value: JsonValue::Null,
        }),
    })
}

/// Build one identity-bound isolated fit receipt.
fn fixture_fit() -> NativeMaintenanceFitResult {
    let candidate_json = br#"{"family":"rain"}"#.to_vec();
    NativeMaintenanceFitResult {
        candidate_sha256: hash_bytes(&candidate_json),
        candidate_json,
        code_snapshot_sha256: hash_byte(b'b'),
        contract_version: "adjustment-fit-sandbox/v1".to_owned(),
        family: NativeMaintenanceCaptureFamily::Rain,
        input_snapshot_sha256: hash_byte(b'c'),
        runtime_readiness_sha256: hash_byte(b'd'),
        stderr: fixture_fit_log("stderr"),
        stdout: fixture_fit_log("stdout"),
    }
}

/// Hash one repeated fixture byte through the actual native primitive.
fn hash_byte(byte: u8) -> String {
    hash_bytes(&[byte])
}

/// Hash exact fixture bytes through the actual native primitive.
fn hash_bytes(bytes: &[u8]) -> String {
    format_digest(NativeWirePrimitives.sha256(bytes))
}

/// Construct the exact digest-only empty fit log.
fn fixture_fit_log(label: &'static str) -> crate::NativeMaintenanceFitLogReport {
    crate::NativeMaintenanceFitLogReport {
        bytes: 0,
        label,
        overflow: false,
        sha256: format_digest(NativeWirePrimitives.sha256(&[])),
    }
}
