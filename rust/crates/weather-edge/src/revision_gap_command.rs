//! Map existing gap-transfer command operands without consulting a deployment or shell.

use crate::revision_gap_ack::RevisionGapAckRequest;
use crate::revision_gap_transfer::{RevisionGapPageRequest, RevisionGapSha256};

/// Already parsed operands of the three deployed, versioned transfer commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevisionGapOperatorCommand {
    Start,
    Page(RevisionGapPageRequest),
    Acknowledge(RevisionGapAckRequest),
}

/// Input errors before storage is opened by the command owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevisionGapOperatorArgumentError {
    StartArity,
    PageArity,
    AcknowledgementArity,
    StartDigest,
    FrontierDigest,
    PageDigest,
    GraphDigest,
}

/// Parse only the gap command family, leaving other operator verbs to their owners.
pub fn parse_revision_gap_operator_command(
    action: &str,
    arguments: &[&str],
) -> Result<Option<RevisionGapOperatorCommand>, RevisionGapOperatorArgumentError> {
    use RevisionGapOperatorArgumentError as Error;
    let command = match (action, arguments) {
        ("revision-gap-start-v1", []) => RevisionGapOperatorCommand::Start,
        ("revision-gap-start-v1", _) => return Err(Error::StartArity),
        ("revision-gap-page-v1", [start, frontier]) => {
            RevisionGapOperatorCommand::Page(RevisionGapPageRequest {
                start_sha256: RevisionGapSha256::from_lower_hex(start).ok_or(Error::StartDigest)?,
                frontier_sha256: RevisionGapSha256::from_lower_hex(frontier)
                    .ok_or(Error::FrontierDigest)?,
            })
        }
        ("revision-gap-page-v1", _) => return Err(Error::PageArity),
        ("revision-gap-ack-v1", [page, graph]) => {
            RevisionGapOperatorCommand::Acknowledge(RevisionGapAckRequest {
                page_sha256: RevisionGapSha256::from_lower_hex(page).ok_or(Error::PageDigest)?,
                graph_manifest_sha256: RevisionGapSha256::from_lower_hex(graph)
                    .ok_or(Error::GraphDigest)?,
            })
        }
        ("revision-gap-ack-v1", _) => return Err(Error::AcknowledgementArity),
        _ => return Ok(None),
    };
    Ok(Some(command))
}
