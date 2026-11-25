//! Typed cold-custody checkpoint identity and exact retained command operands.

use weather_contracts::JsonValue;

use crate::revision_archive_canonical::RevisionArchiveCanonicalCodec;
use crate::revision_cold_page::{
    RevisionColdCursor, RevisionColdPageRequest, parse_revision_cold_page_command,
};
use crate::revision_gap_transfer::RevisionGapSha256;

/// Maximum size of either retained custody checkpoint.
pub const REVISION_COLD_ACK_MAXIMUM_BYTES: usize = 16 * 1024;

/// The caller's original versioned workstation custody identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevisionColdAckIdentity {
    GraphManifest(RevisionGapSha256),
    CustodyCheckpoint(RevisionGapSha256),
}

impl RevisionColdAckIdentity {
    /// Return the exact contract literal for the selected original command.
    pub const fn contract_version(self) -> &'static str {
        match self {
            Self::GraphManifest(_) => "adjustment-revision-custody-acknowledgement/v1",
            Self::CustodyCheckpoint(_) => "adjustment-revision-custody-acknowledgement/v2",
        }
    }

    /// Report whether the packed metadata proof transition applies.
    pub const fn packed(self) -> bool {
        matches!(self, Self::CustodyCheckpoint(_))
    }
}

/// Exact ten operands of either successful cold-page custody command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RevisionColdAckRequest {
    pub page: RevisionColdPageRequest,
    pub page_sha256: RevisionGapSha256,
    pub identity: RevisionColdAckIdentity,
    pub member_root_sha256: RevisionGapSha256,
    pub start_member_sha256: Option<RevisionGapSha256>,
}

/// The original closed successful-spool member classes in wire sort order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RevisionColdRetirementKind {
    RainControlState,
    RainControlStateStageReceipt,
    RevisionCommitGroup,
    RevisionCommitReceipt,
    RevisionFrontierSuccessor,
    RevisionProjection,
    RevisionPublishReceipt,
    RevisionStageReceipt,
    RevisionTerminalGap,
    ShadowRevisionCapsule,
    ShadowRevisionPublishReceipt,
    ShadowRevisionTerminalGap,
}

impl RevisionColdRetirementKind {
    /// Map the fixed file class to its original wire name.
    pub const fn wire_value(self) -> &'static str {
        match self {
            Self::RainControlState => "rain_control_state",
            Self::RainControlStateStageReceipt => "rain_control_state_stage_receipt",
            Self::RevisionCommitGroup => "revision_commit_group",
            Self::RevisionCommitReceipt => "revision_commit_receipt",
            Self::RevisionFrontierSuccessor => "revision_frontier_successor",
            Self::RevisionProjection => "revision_projection",
            Self::RevisionPublishReceipt => "revision_publish_receipt",
            Self::RevisionStageReceipt => "revision_stage_receipt",
            Self::RevisionTerminalGap => "revision_terminal_gap",
            Self::ShadowRevisionCapsule => "shadow_revision_capsule",
            Self::ShadowRevisionPublishReceipt => "shadow_revision_publish_receipt",
            Self::ShadowRevisionTerminalGap => "shadow_revision_terminal_gap",
        }
    }

    /// Admit only the original closed wire names.
    pub fn from_wire_value(value: &str) -> Option<Self> {
        Some(match value {
            "rain_control_state" => Self::RainControlState,
            "rain_control_state_stage_receipt" => Self::RainControlStateStageReceipt,
            "revision_commit_group" => Self::RevisionCommitGroup,
            "revision_commit_receipt" => Self::RevisionCommitReceipt,
            "revision_frontier_successor" => Self::RevisionFrontierSuccessor,
            "revision_projection" => Self::RevisionProjection,
            "revision_publish_receipt" => Self::RevisionPublishReceipt,
            "revision_stage_receipt" => Self::RevisionStageReceipt,
            "revision_terminal_gap" => Self::RevisionTerminalGap,
            "shadow_revision_capsule" => Self::ShadowRevisionCapsule,
            "shadow_revision_publish_receipt" => Self::ShadowRevisionPublishReceipt,
            "shadow_revision_terminal_gap" => Self::ShadowRevisionTerminalGap,
            _ => return None,
        })
    }
}

/// One exact redundant file authorized by the immutable successful page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RevisionColdRetirementEntry {
    pub kind: RevisionColdRetirementKind,
    pub identity_sha256: RevisionGapSha256,
    pub file_sha256: RevisionGapSha256,
}

/// Validated current-page commitments derived from actual archived member bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct RevisionColdAckPage {
    pub packed_shadow_entries: Option<Vec<JsonValue>>,
    pub request: RevisionColdPageRequest,
    pub next: RevisionColdCursor,
    pub page_sha256: RevisionGapSha256,
    pub entry_count: usize,
    pub member_root_sha256: RevisionGapSha256,
    pub retirement_entries: Vec<RevisionColdRetirementEntry>,
}

/// One complete cold-custody-only durable checkpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevisionColdAck {
    pub acknowledged_at: String,
    pub request: RevisionColdAckRequest,
    pub next: RevisionColdCursor,
    pub previous_acknowledgement_sha256: Option<RevisionGapSha256>,
    pub acknowledgement_sha256: RevisionGapSha256,
    pub retirement_entries: Vec<RevisionColdRetirementEntry>,
}

/// Failures at the exact bounded acknowledgement transaction stages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevisionColdAckError {
    Arguments,
    Input,
    Storage,
    State,
    Census,
    Collision,
    Page,
    MemberRoot,
    Cursor,
    Encoding,
    MetadataProof,
    Retirement,
}

/// Parse both original custody commands without accessing any archive files.
pub fn parse_revision_cold_ack_command(
    action: &str,
    operands: &[&str],
) -> Result<Option<RevisionColdAckRequest>, RevisionColdAckError> {
    use RevisionColdAckError as Error;
    let packed = match action {
        "revision-cold-custody-ack-v1" => false,
        "revision-cold-custody-ack-v2" => true,
        _ => return Ok(None),
    };
    // reject the entire operand count before slicing the common cursor
    if operands.len() != 10 {
        return Err(Error::Arguments);
    }
    let page = parse_revision_cold_page_command("revision-cold-page-v1", &operands[..6])
        .map_err(|_| Error::Arguments)?
        .ok_or(Error::Arguments)?;
    let hash = |text: &str| RevisionGapSha256::from_lower_hex(text).ok_or(Error::Input);
    let identity = hash(operands[7])?;
    Ok(Some(RevisionColdAckRequest {
        page,
        page_sha256: hash(operands[6])?,
        identity: if packed {
            RevisionColdAckIdentity::CustodyCheckpoint(identity)
        } else {
            RevisionColdAckIdentity::GraphManifest(identity)
        },
        member_root_sha256: hash(operands[8])?,
        start_member_sha256: match operands[9] {
            "none" => None,
            value => Some(hash(value)?),
        },
    }))
}

/// Classify the same durable identity as a retry without rereading deleted page files.
pub(crate) fn cold_ack_retry(
    request: RevisionColdAckRequest,
    current: Option<&RevisionColdAck>,
) -> Result<bool, RevisionColdAckError> {
    let Some(current) = current else {
        return Ok(false);
    };
    // compare every original caller identity on a repeated page hash
    if current.request.page_sha256 != request.page_sha256 {
        return Ok(false);
    }
    if current.request != request {
        return Err(RevisionColdAckError::Collision);
    }
    Ok(true)
}

/// Require the exact observed page, member root, and acknowledged direct cursor.
pub(crate) fn verify_cold_ack_page(
    request: RevisionColdAckRequest,
    page: &RevisionColdAckPage,
    current: Option<&RevisionColdAck>,
    codec: &impl RevisionArchiveCanonicalCodec,
) -> Result<(), RevisionColdAckError> {
    use RevisionColdAckError as Error;
    // accept only the same nonempty page and first-page start member boundary
    if page.request != request.page
        || page.page_sha256 != request.page_sha256
        || page.entry_count == 0
        || (request.page.previous == request.page.start) != request.start_member_sha256.is_some()
    {
        return Err(Error::Page);
    }
    // compare the caller's archived member population before publishing any ACK
    if page.member_root_sha256 != request.member_root_sha256 {
        return Err(Error::MemberRoot);
    }
    let expected_after = current.map_or_else(
        // hash only the fixed original genesis preimage when no ACK exists
        || RevisionColdCursor {
            ordinal: 0,
            frontier: RevisionGapSha256::admitted(
                codec.sha256(b"adjustment-revision-frontier/v1\n0\n"),
            ),
        },
        |ack| ack.next,
    );
    // allow a new frozen transfer but require one direct acknowledged cursor
    if page.request.after != expected_after
        || current.is_some_and(|ack| {
            page.request.start == ack.request.page.start
                && page.request.previous != ack.request.page_sha256
        })
    {
        return Err(Error::Cursor);
    }
    Ok(())
}

/// The packed v2 transaction's exact metadata proof owner.
///
/// A real owner must prepare and commit the retained proof; absent ownership is
/// an explicit refusal before the corresponding checkpoint is published.
pub trait RevisionColdMetadataProof {
    /// Durably prepare the exact shadow capsule metadata before publishing an ACK.
    fn prepare_cold_metadata_proof(
        &mut self,
        entries: Vec<JsonValue>,
        acknowledgement: &RevisionColdAck,
    ) -> Result<(), RevisionColdAckError>;

    /// Publish or resume the proof only after the bound acknowledgement is durable.
    fn commit_cold_metadata_proof(
        &mut self,
        acknowledgement_sha256: RevisionGapSha256,
    ) -> Result<(), RevisionColdAckError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    // create an explicit ordinary fixture identity
    fn hash(tag: u8) -> RevisionGapSha256 {
        RevisionGapSha256::admitted([tag; 32])
    }

    #[test]
    // preserve the common original ten-operand ordering and v1/v2 identity field
    fn parses_both_original_custody_verbs() {
        let digest = "02".repeat(32);
        let operands = [
            "17", &digest, &digest, "4", &digest, &digest, &digest, &digest, &digest, "none",
        ];
        let v1 = parse_revision_cold_ack_command("revision-cold-custody-ack-v1", &operands)
            .unwrap()
            .unwrap();
        assert_eq!(v1.identity, RevisionColdAckIdentity::GraphManifest(hash(2)));
        assert_eq!(v1.page.watermark.ordinal, 17);
        assert_eq!(v1.page.after.ordinal, 4);
        assert_eq!(v1.start_member_sha256, None);
        let v2 = parse_revision_cold_ack_command("revision-cold-custody-ack-v2", &operands)
            .unwrap()
            .unwrap();
        assert_eq!(
            v2.identity,
            RevisionColdAckIdentity::CustodyCheckpoint(hash(2))
        );
        assert_eq!(
            parse_revision_cold_ack_command("revision-cold-page-v1", &[]),
            Ok(None)
        );
    }

    #[test]
    // confirm the same already durable receipt can retry without its retired files
    fn retry_requires_exact_original_identity() {
        let cursor = RevisionColdCursor {
            ordinal: 1,
            frontier: hash(1),
        };
        let request = RevisionColdAckRequest {
            page: RevisionColdPageRequest {
                after: cursor,
                watermark: cursor,
                start: hash(2),
                previous: hash(3),
            },
            page_sha256: hash(4),
            identity: RevisionColdAckIdentity::GraphManifest(hash(5)),
            member_root_sha256: hash(6),
            start_member_sha256: None,
        };
        let current = RevisionColdAck {
            acknowledged_at: "2026-01-01T00:00:00.000Z".to_owned(),
            request,
            next: cursor,
            previous_acknowledgement_sha256: None,
            acknowledgement_sha256: hash(7),
            retirement_entries: vec![],
        };
        assert_eq!(cold_ack_retry(request, Some(&current)), Ok(true));
        assert_eq!(
            cold_ack_retry(
                RevisionColdAckRequest {
                    page_sha256: hash(8),
                    ..request
                },
                Some(&current)
            ),
            Ok(false)
        );
        assert_eq!(
            cold_ack_retry(
                RevisionColdAckRequest {
                    member_root_sha256: hash(8),
                    ..request
                },
                Some(&current)
            ),
            Err(RevisionColdAckError::Collision)
        );
    }

    struct GenesisCodec;

    impl RevisionArchiveCanonicalCodec for GenesisCodec {
        // the cursor test requires no clock operation
        fn canonical_utc_milliseconds(&self, _: &str) -> Option<i64> {
            None
        }
        // bind only the original fixed genesis preimage for this cursor fixture
        fn sha256(&self, bytes: &[u8]) -> [u8; 32] {
            assert_eq!(bytes, b"adjustment-revision-frontier/v1\n0\n");
            [1; 32]
        }
    }

    #[test]
    // retain exact next-cursor and transfer restart semantics
    fn first_ack_and_new_transfer_follow_the_previous_cursor() {
        let genesis = RevisionColdCursor {
            ordinal: 0,
            frontier: hash(1),
        };
        let first_next = RevisionColdCursor {
            ordinal: 2,
            frontier: hash(2),
        };
        let watermark = RevisionColdCursor {
            ordinal: 6,
            frontier: hash(6),
        };
        let first = RevisionColdAckRequest {
            page: RevisionColdPageRequest {
                after: genesis,
                watermark,
                start: hash(3),
                previous: hash(3),
            },
            page_sha256: hash(4),
            identity: RevisionColdAckIdentity::GraphManifest(hash(5)),
            member_root_sha256: hash(6),
            start_member_sha256: Some(hash(3)),
        };
        let first_page = RevisionColdAckPage {
            packed_shadow_entries: Some(Vec::new()),
            request: first.page,
            next: first_next,
            page_sha256: first.page_sha256,
            entry_count: 1,
            member_root_sha256: first.member_root_sha256,
            retirement_entries: vec![],
        };
        assert_eq!(
            verify_cold_ack_page(first, &first_page, None, &GenesisCodec),
            Ok(())
        );
        let current = RevisionColdAck {
            acknowledged_at: "2026-01-01T00:00:00.000Z".to_owned(),
            request: first,
            next: first_next,
            previous_acknowledgement_sha256: None,
            acknowledgement_sha256: hash(8),
            retirement_entries: vec![],
        };
        let same_transfer = RevisionColdAckRequest {
            page: RevisionColdPageRequest {
                after: first_next,
                previous: first.page_sha256,
                ..first.page
            },
            page_sha256: hash(9),
            start_member_sha256: None,
            ..first
        };
        let second_page = RevisionColdAckPage {
            request: same_transfer.page,
            next: watermark,
            page_sha256: same_transfer.page_sha256,
            ..first_page.clone()
        };
        assert_eq!(
            verify_cold_ack_page(same_transfer, &second_page, Some(&current), &GenesisCodec),
            Ok(())
        );
        let new_transfer = RevisionColdAckRequest {
            page: RevisionColdPageRequest {
                start: hash(10),
                previous: hash(10),
                ..same_transfer.page
            },
            start_member_sha256: Some(hash(11)),
            ..same_transfer
        };
        let restarted_page = RevisionColdAckPage {
            request: new_transfer.page,
            ..second_page
        };
        assert_eq!(
            verify_cold_ack_page(new_transfer, &restarted_page, Some(&current), &GenesisCodec),
            Ok(())
        );
    }
}
