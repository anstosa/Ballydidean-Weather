//! Fixed owner terminal record request and exact predecessor-bound response.

use weather_contracts::JsonValue;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};

use crate::{
    NativeMaintenanceDatabaseLedgerReader, NativeMaintenanceInstalledTerminalMaterial,
    NativeMaintenanceTerminalResultDisposition, NativeWirePrimitives,
    maintenance_canonical_json::{exact_keys, field, text, valid_hash},
    native_maintenance_policy_comparison::{object, string},
    native_maintenance_retirement_files_v2::format_digest,
};

const NO_ACTION_IDENTITY: &str = "adjustment-terminal-no-action-identity/v3";
const NO_ACTION_RECEIPT: &str = "adjustment-terminal-no-action-receipt/v3";

/// Exact no-action identity and receipt retained before owner reconciliation.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceTerminalNoActionReceipt {
    pub identity: JsonValue,
    pub identity_sha256: String,
    pub receipt: JsonValue,
}

/// Borrow the original complete/unsupported proof and actual candidate owner inputs.
pub struct NativeMaintenanceTerminalRecordInput<'a> {
    pub material: &'a NativeMaintenanceInstalledTerminalMaterial,
    pub action: Option<&'a JsonValue>,
    pub action_receipt: &'a JsonValue,
    pub authority_proof: &'a JsonValue,
    pub local_result: &'a JsonValue,
    pub previous_tombstone: Option<&'a JsonValue>,
    pub source_commit: &'a str,
    pub terminal_graph_manifest_sha256: &'a str,
    pub terminal_graph_verified_at: &'a str,
    pub unsupported: bool,
}

/// Frozen canonical request and its content operand.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceTerminalRecordRequest {
    pub request: JsonValue,
    pub canonical_bytes: Vec<u8>,
    pub request_sha256: String,
    pub unsupported: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceTerminalRecordRemoteError {
    Input,
    Clock,
    Transport,
    Response,
}

use NativeMaintenanceTerminalRecordRemoteError as Error;

/// Derive the original nonmutating identity independently of later receipt timestamps.
pub fn build_native_maintenance_terminal_no_action_identity(
    disposition: NativeMaintenanceTerminalResultDisposition,
    policy_sha256: &str,
    registration_sha256: &str,
) -> Result<JsonValue, Error> {
    // preserve only the source's closed nonpromotion dispositions
    if disposition == NativeMaintenanceTerminalResultDisposition::Promoted
        || !valid_hash(policy_sha256)
        || !valid_hash(registration_sha256)
    {
        return Err(Error::Input);
    }
    Ok(object(vec![
        ("contractVersion", string(NO_ACTION_IDENTITY)),
        ("disposition", string(disposition.wire())),
        ("policyReportSha256", string(policy_sha256)),
        ("registrationSha256", string(registration_sha256)),
    ]))
}

/// Build the original verified no-action terminal receipt after actual C/T/F finalization.
pub fn build_native_maintenance_terminal_no_action_receipt(
    disposition: NativeMaintenanceTerminalResultDisposition,
    policy_sha256: &str,
    registration_sha256: &str,
    full_member_root_sha256: &str,
    completed_at: &str,
    finalized_at: &str,
) -> Result<NativeMaintenanceTerminalNoActionReceipt, Error> {
    let identity = build_native_maintenance_terminal_no_action_identity(
        disposition,
        policy_sha256,
        registration_sha256,
    )?;
    let identity_sha256 = hash(&encode(&identity)?);
    if !valid_hash(full_member_root_sha256) || instant(completed_at)? < instant(finalized_at)? {
        return Err(Error::Input);
    }
    let receipt = object(vec![
        ("actionSha256", string(&identity_sha256)),
        ("completedAt", string(completed_at)),
        ("contractVersion", string(NO_ACTION_RECEIPT)),
        ("disposition", string(disposition.wire())),
        ("fullMemberRootSha256", string(full_member_root_sha256)),
        ("policyReportSha256", string(policy_sha256)),
        ("registrationSha256", string(registration_sha256)),
        ("state", string("verified_no_action")),
    ]);
    Ok(NativeMaintenanceTerminalNoActionReceipt {
        identity,
        identity_sha256,
        receipt,
    })
}

/// Bind the actual terminal record request to the original installed proof and archive coordinates.
pub fn build_native_maintenance_terminal_record_request(
    input: NativeMaintenanceTerminalRecordInput<'_>,
) -> Result<NativeMaintenanceTerminalRecordRequest, Error> {
    let material = input.material;
    let shadow = &material.access.shadow_registration.registration;
    let registration = text(shadow, "registrationSha256").ok_or(Error::Input)?;
    let finalization_kind = if input.unsupported {
        "adjustment-maintenance-unsupported-terminal-proof/v1"
    } else {
        "adjustment-maintenance-finalization-proof/v3"
    };
    // rebind the genuine live owner proof, burned access and local result before transport
    if !valid_hash(input.terminal_graph_manifest_sha256)
        || input.source_commit.len() != 40
        || text(input.authority_proof, "contractVersion").as_deref() != Some(finalization_kind)
        || text(input.authority_proof, "graphManifestSha256").as_deref()
            != Some(input.terminal_graph_manifest_sha256)
        || text(input.authority_proof, "sourceCommit").as_deref() != Some(input.source_commit)
        || field(input.authority_proof, "confirmationAccessSha256")
            != field(&material.access.native_access, "accessSha256")
        || field(input.authority_proof, "fullMemberRootSha256")
            != field(&material.full_manifest, "fullMemberRootSha256")
        || field(input.local_result, "registrationSha256")
            != field(
                &material.access.confirmation_registration,
                "registrationSha256",
            )
        || field(input.local_result, "fullMemberRootSha256")
            != field(&material.full_manifest, "fullMemberRootSha256")
        || field(input.authority_proof, "actionSha256")
            != field(input.action_receipt, "actionSha256")
        || !valid_hash(&registration)
        || (input.unsupported && input.action.is_some())
    {
        return Err(Error::Input);
    }
    instant(input.terminal_graph_verified_at)?;
    // require canonical predecessor tombstone identity, if the family already retained one
    if let Some(previous) = input.previous_tombstone
        && (!exact_keys(
            previous,
            &[
                "contractVersion",
                "reconciliationSha256",
                "registrationSha256",
                "terminalResultSha256",
            ],
        ) || text(previous, "contractVersion").as_deref()
            != Some("adjustment-shadow-terminal-tombstone/v3")
            || [
                "reconciliationSha256",
                "registrationSha256",
                "terminalResultSha256",
            ]
            .iter()
            .any(|key| text(previous, key).is_none_or(|v| !valid_hash(&v))))
    {
        return Err(Error::Input);
    }
    let mut fields = vec![
        ("actionReceipt", input.action_receipt.clone()),
        (
            "confirmationRegistration",
            material.access.confirmation_registration.clone(),
        ),
        (
            "contractVersion",
            string(if input.unsupported {
                "adjustment-shadow-unsupported-terminal-record-request/v1"
            } else {
                "adjustment-shadow-terminal-record-request/v3"
            }),
        ),
        ("localResult", input.local_result.clone()),
        ("nativeAccess", material.access.native_access.clone()),
        (
            "previousTombstone",
            input.previous_tombstone.cloned().unwrap_or(JsonValue::Null),
        ),
        ("shadowRegistration", shadow.clone()),
        ("sourceCommit", string(input.source_commit)),
        (
            "terminalGraphManifestSha256",
            string(input.terminal_graph_manifest_sha256),
        ),
        (
            "terminalGraphVerifiedAt",
            string(input.terminal_graph_verified_at),
        ),
    ];
    // unsupported and complete requests are separate forced owner contracts
    if input.unsupported {
        fields.push(("unsupportedProof", input.authority_proof.clone()));
    } else {
        fields.push(("action", input.action.cloned().unwrap_or(JsonValue::Null)));
        fields.push(("finalizationProof", input.authority_proof.clone()));
    }
    let request = object(fields);
    let canonical_bytes = encode(&request)?;
    Ok(NativeMaintenanceTerminalRecordRequest {
        request,
        request_sha256: hash(&canonical_bytes),
        canonical_bytes,
        unsupported: input.unsupported,
    })
}

/// Record only the typed installed terminal request through its fixed original command.
pub fn record_native_maintenance_terminal_remote(
    remote: &NativeMaintenanceDatabaseLedgerReader,
    request: &NativeMaintenanceTerminalRecordRequest,
) -> Result<JsonValue, Error> {
    let bytes = encode(&request.request)?;
    if bytes != request.canonical_bytes || hash(&bytes) != request.request_sha256 {
        return Err(Error::Input);
    }
    let operation = if request.unsupported {
        "adjustment-shadow-unsupported-terminal-record-v1"
    } else {
        "adjustment-shadow-terminal-record-v3"
    };
    let response = remote
        .request_document(
            operation,
            &[&request.request_sha256],
            Some(&request.canonical_bytes),
        )
        .map_err(|_| Error::Transport)?;
    verify_native_maintenance_terminal_record_response(request, &response)
}

/// Verify the authenticated response envelope and exact retained terminal row.
pub fn verify_native_maintenance_terminal_record_response(
    request: &NativeMaintenanceTerminalRecordRequest,
    bytes: &[u8],
) -> Result<JsonValue, Error> {
    let response = NativeWirePrimitives
        .decode_json(bytes)
        .ok_or(Error::Response)?;
    if encode(&response)? != bytes
        || !exact_keys(
            &response,
            &[
                "contractVersion",
                "reconciliationSha256",
                "registrationSha256",
                "state",
                "terminalRecord",
            ],
        )
        || text(&response, "contractVersion").as_deref()
            != Some("adjustment-shadow-terminal-record-result/v3")
        || text(&response, "state").as_deref() != Some("recorded")
    {
        return Err(Error::Response);
    }
    let terminal = field(&response, "terminalRecord").ok_or(Error::Response)?;
    crate::native_maintenance_terminal_completion_owner::validate_terminal_record(terminal)
        .ok_or(Error::Response)?;
    let proof = field(
        &request.request,
        if request.unsupported {
            "unsupportedProof"
        } else {
            "finalizationProof"
        },
    )
    .ok_or(Error::Input)?;
    let native_access = field(&request.request, "nativeAccess").ok_or(Error::Input)?;
    let registration = field(&request.request, "shadowRegistration").ok_or(Error::Input)?;
    let expected_authority = if request.unsupported {
        hash(&encode(proof)?)
    } else {
        text(proof, "transferredAnchorSha256").ok_or(Error::Input)?
    };
    // crossbind the row to the same owner registration, proof, action and terminal member
    if field(terminal, "reconciliationSha256") != field(&response, "reconciliationSha256")
        || field(terminal, "registrationSha256") != field(&response, "registrationSha256")
        || field(terminal, "registrationSha256") != field(registration, "registrationSha256")
        || field(terminal, "family") != field(registration, "family")
        || field(terminal, "accessSha256") != field(native_access, "accessSha256")
        || field(terminal, "actionSha256") != field(proof, "actionSha256")
        || field(terminal, "terminalMemberSha256") != field(proof, "fullMemberRootSha256")
        || text(terminal, "maintenanceAnchorSha256").as_deref() != Some(&expected_authority)
    {
        return Err(Error::Response);
    }
    Ok(terminal.clone())
}

/// Encode the source's canonical LF-terminated JSON wire representation.
fn encode(value: &JsonValue) -> Result<Vec<u8>, Error> {
    NativeWirePrimitives
        .encode_sorted_json(value)
        .ok_or(Error::Input)
}

/// Digest one exact source document.
fn hash(bytes: &[u8]) -> String {
    format_digest(RevisionArchiveCanonicalCodec::sha256(
        &NativeWirePrimitives,
        bytes,
    ))
}

/// Parse one actual canonical clock.
fn instant(value: &str) -> Result<i64, Error> {
    NativeWirePrimitives
        .canonical_utc_milliseconds(value)
        .ok_or(Error::Clock)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::Write,
        path::Path,
        process::{Command, Stdio},
    };

    #[test]
    fn no_action_receipt_matches_original_source() {
        let policy = "a".repeat(64);
        let registration = "b".repeat(64);
        let root = "c".repeat(64);
        let at = "2026-11-01T12:00:00.000Z";
        let result = build_native_maintenance_terminal_no_action_receipt(
            NativeMaintenanceTerminalResultDisposition::SupportFailed,
            &policy,
            &registration,
            &root,
            at,
            at,
        )
        .unwrap();
        let input = object(vec![
            ("completedAt", string(at)),
            ("disposition", string("support_failed")),
            ("finalizedAt", string(at)),
            ("fullMemberRootSha256", string(&root)),
            ("policyReportSha256", string(&policy)),
            ("registrationSha256", string(&registration)),
        ]);
        let source = r#"
import { readFileSync } from "node:fs";
import { buildAdjustmentTerminalNoActionReceipt } from "./scripts/research/adjustment_maintenance_controller.mjs";
const result=buildAdjustmentTerminalNoActionReceipt(JSON.parse(readFileSync(0,"utf8")));
process.stdout.write(JSON.stringify({identity:result.identityBytes.toString("base64"),receipt:result.receiptBytes.toString("base64")}));
"#;
        let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let mut child = Command::new("node")
            .args(["--input-type=module", "--eval", source])
            .current_dir(repository)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(&encode(&input).unwrap())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "source oracle: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let expected = NativeWirePrimitives.decode_json(&output.stdout).unwrap();
        let bytes = |name| {
            NativeWirePrimitives
                .decode_standard_base64(&text(&expected, name).unwrap())
                .unwrap()
        };
        assert_eq!(encode(&result.identity).unwrap(), bytes("identity"));
        assert_eq!(encode(&result.receipt).unwrap(), bytes("receipt"));
    }
}
