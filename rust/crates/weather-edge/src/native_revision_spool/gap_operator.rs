//! Bind the native permanent-gap commands to the existing canonical I/O owners.

use std::io::{self, Write};

use crate::revision_archive_canonical::RevisionArchiveCanonicalClock;
use crate::revision_gap_ack::{
    RevisionGapAckCodec, RevisionGapAckError, RevisionGapRetirementDirectory as Directory,
    RevisionGapRetirementGroup, RevisionGapRetirementMember, acknowledge_revision_gap_payload,
};
use crate::revision_gap_command::RevisionGapOperatorCommand;
use crate::revision_gap_transfer::{
    REVISION_GAP_PROJECTION_MAXIMUM_BYTES, REVISION_GAP_RAIN_CONTROL_STATE_MAXIMUM_BYTES,
    REVISION_GAP_RECEIPT_MAXIMUM_BYTES, REVISION_GAP_SHADOW_STAGE_MAXIMUM_BYTES, RevisionGapSha256,
    RevisionGapTransferStart,
};

use super::gap_ack::{NativeRevisionGapAckDecoder, NativeRevisionGapAckStorage};
use super::gap_transfer::{
    NativeRevisionGapMember, NativeRevisionGapPayloadKind as Kind, NativeRevisionGapTransferCodec,
    NativeRevisionGapTransferError,
};
use super::{NativeRevisionSpool, RevisionSpoolLeaseCodec};

/// Canonical operations used by the selected native command producer.
pub trait NativeRevisionGapCommandCodec:
    NativeRevisionGapTransferCodec + RevisionGapAckCodec<NativeRevisionGapMember>
{
    /// Emit the exact canonical start envelope including its previously derived digest.
    fn encode_start(&self, start: &RevisionGapTransferStart) -> Option<Vec<u8>>;
}

/// Bind separately borrowed canonical readers and the acknowledgement time owner.
pub struct NativeRevisionGapCommandPorts<'a, Codec, Decoder, Clock> {
    pub codec: &'a mut Codec,
    pub ack_decoder: &'a Decoder,
    pub clock: &'a mut Clock,
}

/// Failures below the deployment command binding, excluding shell and transport.
#[derive(Debug)]
pub enum NativeRevisionGapCommandError {
    Transfer(NativeRevisionGapTransferError),
    Acknowledge(RevisionGapAckError),
    OutputEncoding,
    Output(io::Error),
}

impl<Lease: RevisionSpoolLeaseCodec> NativeRevisionSpool<Lease> {
    /// Execute one already selected command and emit exactly one canonical response to its owner.
    pub fn run_gap_operator_command<Codec, Decoder, Clock>(
        &mut self,
        command: RevisionGapOperatorCommand,
        ports: &mut NativeRevisionGapCommandPorts<'_, Codec, Decoder, Clock>,
        output: &mut impl Write,
    ) -> Result<(), NativeRevisionGapCommandError>
    where
        Codec: NativeRevisionGapCommandCodec,
        Decoder: NativeRevisionGapAckDecoder,
        Clock: RevisionArchiveCanonicalClock,
    {
        use NativeRevisionGapCommandError as Error;
        let bytes = match command {
            RevisionGapOperatorCommand::Start => {
                let start = self
                    .read_gap_transfer_start(ports.codec)
                    .map_err(Error::Transfer)?;
                ports
                    .codec
                    .encode_start(&start)
                    .ok_or(Error::OutputEncoding)?
            }
            RevisionGapOperatorCommand::Page(request) => {
                let (_, bytes) = self
                    .resolve_gap_payload_page(request, ports.codec)
                    .map_err(Error::Transfer)?;
                bytes
            }
            RevisionGapOperatorCommand::Acknowledge(request) => {
                let mut storage = NativeRevisionGapAckStorage::new(self, ports.ack_decoder);
                let ack = acknowledge_revision_gap_payload(
                    &mut storage,
                    ports.codec,
                    ports.clock,
                    request,
                )
                .map_err(Error::Acknowledge)?;
                ports
                    .codec
                    .acknowledgement_bytes(&ack)
                    .ok_or(Error::OutputEncoding)?
            }
        };
        output.write_all(&bytes).map_err(Error::Output)
    }
}

impl NativeRevisionGapMember {
    /// Map one admitted member to the source's ordered verification and retirement groups.
    pub fn retirement_groups(&self) -> Option<Vec<RevisionGapRetirementGroup>> {
        let slot = self.slot_sha256;
        // bind the exact retained counterpart rather than deriving a path from caller text
        match (self.kind, self.rain_control.as_ref()) {
            (Kind::RainControlState, None) => {
                let mut members =
                    rain_state_members(slot, self.payload_sha256, self.stage_receipt_member_sha256);
                members.push(member(
                    Directory::RainControlStateGaps,
                    slot,
                    self.gap_member_sha256,
                    REVISION_GAP_RECEIPT_MAXIMUM_BYTES,
                ));
                Some(vec![RevisionGapRetirementGroup { members }])
            }
            (Kind::ShadowStageV1 | Kind::ShadowStageV2, None) => {
                Some(vec![RevisionGapRetirementGroup {
                    members: vec![member(
                        Directory::ShadowRevisionStages,
                        slot,
                        self.payload_sha256,
                        REVISION_GAP_SHADOW_STAGE_MAXIMUM_BYTES,
                    )],
                }])
            }
            (Kind::RainGateControl, Some(auxiliary)) => Some(vec![
                projection_group(self),
                RevisionGapRetirementGroup {
                    members: rain_state_members(
                        auxiliary.state_sha256,
                        auxiliary.state_sha256,
                        auxiliary.stage_receipt_member_sha256,
                    ),
                },
            ]),
            (
                Kind::Projection
                | Kind::BatchProjection
                | Kind::RainGateFeature
                | Kind::RainFixedGaugeProjection
                | Kind::TemperatureNativeSourceProjection,
                None,
            ) => Some(vec![projection_group(self)]),
            _ => None,
        }
    }
}

/// Keep main projection and original stage receipt in one atomic verification group.
fn projection_group(value: &NativeRevisionGapMember) -> RevisionGapRetirementGroup {
    RevisionGapRetirementGroup {
        members: vec![
            member(
                Directory::RevisionProjections,
                value.slot_sha256,
                value.payload_sha256,
                REVISION_GAP_PROJECTION_MAXIMUM_BYTES,
            ),
            member(
                Directory::RevisionStageReceipts,
                value.slot_sha256,
                value.stage_receipt_member_sha256,
                REVISION_GAP_RECEIPT_MAXIMUM_BYTES,
            ),
        ],
    }
}

/// Preserve the state's exact payload-then-stage order for either source of retirement.
fn rain_state_members(
    slot: RevisionGapSha256,
    state: RevisionGapSha256,
    receipt: RevisionGapSha256,
) -> Vec<RevisionGapRetirementMember> {
    vec![
        member(
            Directory::RainControlStates,
            slot,
            state,
            REVISION_GAP_RAIN_CONTROL_STATE_MAXIMUM_BYTES,
        ),
        member(
            Directory::RainControlStateStageReceipts,
            slot,
            receipt,
            REVISION_GAP_RECEIPT_MAXIMUM_BYTES,
        ),
    ]
}

/// Describe one exact optional immutable file without taking its path as input.
const fn member(
    directory: Directory,
    slot_sha256: RevisionGapSha256,
    expected_sha256: RevisionGapSha256,
    maximum_bytes: usize,
) -> RevisionGapRetirementMember {
    RevisionGapRetirementMember {
        directory,
        expected_sha256,
        maximum_bytes,
        slot_sha256,
    }
}
