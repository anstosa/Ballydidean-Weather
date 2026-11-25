//! Closed mixed controller archive registry for source evidence and retained custody ancestry.

use crate::maintenance_canonical_json::text;
use crate::{MaintenanceCanonicalJson, NativeWirePrimitives};
use regex::Regex;
use std::sync::OnceLock;

use crate::{
    NATIVE_MAINTENANCE_ATTEMPT_MAXIMUM_BYTES, NATIVE_MAINTENANCE_ATTEMPT_VERSION,
    NativeMaintenanceCasMemberValidator, NativeMaintenanceCasStreamSession,
    NativeMaintenanceCasStreamValidator, NativeMaintenanceColdArchiveValidationSession,
    NativeMaintenanceColdArchiveValidators, verify_native_maintenance_attempt_report,
};

/// Source-verified controller member kinds without an unrestricted JSON fallback.
#[derive(Default)]
pub struct NativeMaintenanceControllerArchiveValidators;

impl NativeMaintenanceControllerArchiveValidators {
    /// Construct the closed controller registry.
    pub const fn new() -> Self {
        Self
    }
}

impl NativeMaintenanceCasMemberValidator for NativeMaintenanceControllerArchiveValidators {
    /// Verify an attempt report or one of the exact already supported custody kinds.
    fn validate(&mut self, kind: &str, payload: &[u8]) -> bool {
        if kind == NATIVE_MAINTENANCE_ATTEMPT_VERSION {
            verify_native_maintenance_attempt_report(payload).is_some()
        } else if confirmation_plan_kind(kind).is_some() {
            crate::validate_native_maintenance_confirmation_plan_graph_bytes(kind, payload)
                .is_some()
        } else if crate::native_maintenance_archive_registration_maximum(kind).is_some() {
            crate::validate_native_maintenance_archive_registration(kind, payload).is_some()
        } else if let Some((kind, maximum)) = source_member_kind(kind) {
            validate_source_member(kind, payload, maximum)
        } else {
            NativeMaintenanceColdArchiveValidators::new().validate(kind, payload)
        }
    }
}

impl NativeMaintenanceCasStreamValidator for NativeMaintenanceControllerArchiveValidators {
    type Session = NativeMaintenanceControllerArchiveValidationSession;
    /// Open only a bounded admitted member kind.
    fn begin(&mut self, kind: &str, length: u64) -> Option<Self::Session> {
        if kind == NATIVE_MAINTENANCE_ATTEMPT_VERSION {
            let length = usize::try_from(length).ok()?;
            if length == 0 || length > NATIVE_MAINTENANCE_ATTEMPT_MAXIMUM_BYTES {
                return None;
            }
            Some(
                NativeMaintenanceControllerArchiveValidationSession::Attempt {
                    bytes: Vec::with_capacity(length),
                    length,
                },
            )
        } else if let Some((kind, maximum)) =
            crate::native_maintenance_archive_registration_maximum(kind)
                .or_else(|| confirmation_plan_kind(kind))
                .or_else(|| source_member_kind(kind))
        {
            let length = usize::try_from(length).ok()?;
            // preserve the original source member-specific bound
            if length == 0 || length > maximum {
                return None;
            }
            Some(
                NativeMaintenanceControllerArchiveValidationSession::Source {
                    kind,
                    bytes: Vec::with_capacity(length),
                    length,
                    maximum,
                },
            )
        } else {
            Some(NativeMaintenanceControllerArchiveValidationSession::Cold(
                NativeMaintenanceColdArchiveValidators::new().begin(kind, length)?,
            ))
        }
    }
}

/// Bounded source report validation or the exact delegated custody validation.
pub enum NativeMaintenanceControllerArchiveValidationSession {
    Attempt {
        bytes: Vec<u8>,
        length: usize,
    },
    Source {
        kind: &'static str,
        bytes: Vec<u8>,
        length: usize,
        maximum: usize,
    },
    Cold(NativeMaintenanceColdArchiveValidationSession),
}

impl NativeMaintenanceCasStreamSession for NativeMaintenanceControllerArchiveValidationSession {
    /// Preserve the advertised bounded member length throughout streaming.
    fn write(&mut self, chunk: &[u8]) -> bool {
        match self {
            Self::Attempt { bytes, length } | Self::Source { bytes, length, .. } => {
                if bytes
                    .len()
                    .checked_add(chunk.len())
                    .is_none_or(|next| next > *length)
                {
                    return false;
                }
                bytes.extend_from_slice(chunk);
                true
            }
            Self::Cold(session) => session.write(chunk),
        }
    }

    /// Accept only a complete canonical source member.
    fn finish(self) -> bool {
        match self {
            Self::Attempt { bytes, length } => {
                bytes.len() == length && verify_native_maintenance_attempt_report(&bytes).is_some()
            }
            Self::Source {
                kind,
                bytes,
                length,
                maximum,
            } => {
                bytes.len() == length
                    && if confirmation_plan_kind(kind).is_some() {
                        crate::validate_native_maintenance_confirmation_plan_graph_bytes(
                            kind, &bytes,
                        )
                        .is_some()
                    } else if crate::native_maintenance_archive_registration_maximum(kind).is_some()
                    {
                        crate::validate_native_maintenance_archive_registration(kind, &bytes)
                            .is_some()
                    } else {
                        validate_source_member(kind, &bytes, maximum)
                    }
            }
            Self::Cold(session) => session.finish(),
        }
    }
}

const EVIDENCE_MAXIMUM: usize = 4_832 * 1024 + 1;
const LARGE_MAXIMUM: usize = 8 * 1024 * 1024;
const CANONICAL_EVIDENCE_KINDS: &[&str] = &[
    "adjustment-archive-graph-reference/v1",
    "adjustment-confirmation-local-burn-member/v1",
    "adjustment-confirmation-member/v3",
    "adjustment-confirmation-native-access-member/v1",
    "adjustment-development-custody-graph/v1",
    "adjustment-maintenance-confirmation-terminal-graph-manifest/v1",
    "adjustment-maintenance-confirmation-terminal-graph-part/v1",
    "adjustment-maintenance-candidate-report/v1",
    "adjustment-maintenance-unsupported-policy-report/v1",
    "adjustment-terminal-no-action-identity/v3",
    "adjustment-shadow-terminal-tombstone/v3",
    "forecast-adjustment-future-only-source-lineage/v1",
    "forecast-adjustment-lifecycle-ledger/v2",
    "forecast-adjustment-maintenance-runtime-package/v1",
    "forecast-adjustment-model-action/v1",
    "forecast-adjustment-model-parity/v1",
    "rain-hurdle-wind-runtime/v1",
    "rain-hurdle-wind-runtime/v2",
    "rain-maintenance-policy/v2",
    "temperature-maintenance-policy/v2",
    "wind-maintenance-policy/v2",
];
const LARGE_EVIDENCE_KINDS: &[&str] = &[
    "adjustment-maintenance-evaluation-chunk/v1",
    "adjustment-maintenance-evaluation-chunk/v2",
    "adjustment-maintenance-evaluation-chunk/v3",
];
const CANDIDATE_KINDS: &[&str] = &[
    "forecast-adjustment-temperature-development-candidate/v1",
    "forecast-adjustment-wind-development-candidate/v1",
    "forecast-adjustment-rain-development-candidate/v1",
];
static PRIVATE_ARCHIVED_CANDIDATE: OnceLock<Option<Regex>> = OnceLock::new();

/// Restrict the canonical evidence validator to the exact original registered member identities.
fn source_member_kind(kind: &str) -> Option<(&'static str, usize)> {
    if let Some(value) = CANONICAL_EVIDENCE_KINDS
        .iter()
        .copied()
        .find(|value| *value == kind)
    {
        return Some((value, EVIDENCE_MAXIMUM));
    }
    LARGE_EVIDENCE_KINDS
        .iter()
        .chain(CANDIDATE_KINDS)
        .copied()
        .find(|value| *value == kind)
        .map(|value| (value, LARGE_MAXIMUM))
}

/// Preserve canonical builder-validated evidence and the original family candidate envelope.
fn validate_source_member(kind: &str, payload: &[u8], maximum: usize) -> bool {
    if payload.len() < 2 || payload.len() > maximum {
        return false;
    }
    let Some(value) = NativeWirePrimitives.parse_canonical_object(payload) else {
        return false;
    };
    let Some(contract) = text(&value, "contractVersion") else {
        return false;
    };
    let candidate_contract = match kind {
        "forecast-adjustment-temperature-development-candidate/v1" => {
            Some(contract == "temperature-maintenance-fit/v2")
        }
        "forecast-adjustment-wind-development-candidate/v1" => {
            Some(contract == "wind-maintenance-fit/v2")
        }
        "forecast-adjustment-rain-development-candidate/v1" => Some(matches!(
            contract.as_str(),
            "rain-maintenance-fit/v2" | "rain-maintenance-fit/v3"
        )),
        _ => None,
    };
    // preserve the predecessor's separate candidate byte filter
    if let Some(admitted) = candidate_contract {
        let Some(pattern) = PRIVATE_ARCHIVED_CANDIDATE.get_or_init(|| Regex::new(r"(?:/home/|/root/|/mnt/c/Users/|SSH_AUTH_SOCK|ANSEL_HOST_PASSWORD|-----BEGIN [A-Z ]*PRIVATE KEY-----)").ok()).as_ref() else {
            return false;
        };
        return admitted
            && std::str::from_utf8(payload).is_ok_and(|source| !pattern.is_match(source));
    }
    contract == kind
}

/// Dispatch the two exact source confirmation plan graph kinds and their shared bound.
fn confirmation_plan_kind(kind: &str) -> Option<(&'static str, usize)> {
    let admitted = match kind {
        crate::NATIVE_CONFIRMATION_PLAN_GRAPH_VERSION => {
            crate::NATIVE_CONFIRMATION_PLAN_GRAPH_VERSION
        }
        crate::NATIVE_UNSUPPORTED_CONFIRMATION_PLAN_GRAPH_VERSION => {
            crate::NATIVE_UNSUPPORTED_CONFIRMATION_PLAN_GRAPH_VERSION
        }
        _ => return None,
    };
    Some((
        admitted,
        crate::NATIVE_CONFIRMATION_PLAN_GRAPH_MAXIMUM_BYTES,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Preserve one native report and old value-bearing chunk through both archive admission paths.
    #[test]
    fn controller_registry_streams_canonical_mixed_history() {
        for kind in [
            "adjustment-maintenance-candidate-report/v1",
            "adjustment-maintenance-evaluation-chunk/v2",
            "forecast-adjustment-model-parity/v1",
        ] {
            let payload = format!("{{\"contractVersion\":\"{kind}\"}}\n");
            let mut registry = NativeMaintenanceControllerArchiveValidators::new();
            assert!(registry.validate(kind, payload.as_bytes()));
            let mut stream = registry
                .begin(kind, payload.len() as u64)
                .expect("source member");
            for chunk in payload.as_bytes().chunks(7) {
                assert!(stream.write(chunk));
            }
            assert!(stream.finish());
        }
    }
}
