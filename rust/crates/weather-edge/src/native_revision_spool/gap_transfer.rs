//! Actual native collection and pending-page retention for the operator gap export.

use std::collections::{BTreeMap, BTreeSet};

use weather_contracts::{
    AdjustmentRevisionProjectionKind, Nullable, RainMaintenanceControlStateGapInput,
    RainMaintenanceControlStateGapInputReasonLiteral,
};

use super::{
    NativeRevisionSpool, NativeRevisionSpoolArtifactCodec, RevisionSpoolLeaseCodec, list_names,
    read_optional_file, read_private_file, require_directory,
};
use crate::permanent_archive_gap::{PermanentArchiveGapInput, bounded_gap_bytes};
use crate::rain_control_state_archive::{
    decode_canonical_rain_gap_receipt, decode_canonical_rain_stage_receipt,
};
use crate::revision_archive_canonical::{is_payload_name, lower_hex};
use crate::revision_gap_transfer::{
    REVISION_GAP_BATCH_COMMIT_RECEIPT_MAXIMUM_BYTES, REVISION_GAP_METADATA_MAXIMUM_FILES,
    REVISION_GAP_ONLINE_MAXIMUM_FILES, REVISION_GAP_PAGE_MAXIMUM_BYTES,
    REVISION_GAP_PROJECTION_MAXIMUM_BYTES, REVISION_GAP_RAIN_CONTROL_STATE_MAXIMUM_BYTES,
    REVISION_GAP_RECEIPT_MAXIMUM_BYTES, REVISION_GAP_SHADOW_STAGE_MAXIMUM_BYTES,
    RetainedRevisionGapEntry, RevisionGapFrontier, RevisionGapIdentityAdapter,
    RevisionGapPageRequest, RevisionGapPayloadPage, RevisionGapRainControlStateBytes,
    RevisionGapRetainedBytes, RevisionGapRetainedCensus, RevisionGapSha256,
    RevisionGapTransferError, RevisionGapTransferStart, read_revision_gap_transfer_start,
    select_new_revision_gap_payload_page, validate_retained_page,
};

const TRANSFER: &str = "revision-gap-transfer";
const COMMITS: &str = "revision-commit-receipts";
const PROJECTIONS: &str = "revision-projections";
const PROJECTION_RECEIPTS: &str = "revision-stage-receipts";
const REVISION_GAPS: &str = "revision-gaps";
const SHADOW_STAGES: &str = "shadow-revision-stages";
const SHADOW_GAPS: &str = "shadow-revision-gaps";
const RAIN_STATES: &str = "rain-control-states";
const RAIN_RECEIPTS: &str = "rain-control-state-stage-receipts";
const RAIN_GAPS: &str = "rain-control-state-gaps";
const GAP_TRANSFER_DIRS: [&str; 11] = [
    COMMITS,
    TRANSFER,
    REVISION_GAPS,
    PROJECTIONS,
    "revision-spool-locks",
    PROJECTION_RECEIPTS,
    SHADOW_GAPS,
    SHADOW_STAGES,
    RAIN_STATES,
    RAIN_RECEIPTS,
    RAIN_GAPS,
];

/// The exact retained projection kind accepted by the deployed gap collector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeRevisionGapPayloadKind {
    Projection,
    BatchProjection,
    RainGateFeature,
    RainGateControl,
    RainFixedGaugeProjection,
    TemperatureNativeSourceProjection,
    RainControlState,
    ShadowStageV1,
    ShadowStageV2,
}

impl NativeRevisionGapPayloadKind {
    /// Return the exact source contract identity.
    pub const fn wire_value(self) -> &'static str {
        match self {
            Self::Projection => "adjustment-revision-projection/v1",
            Self::BatchProjection => "adjustment-revision-batch-projection/v2",
            Self::RainGateFeature => "adjustment-rain-gate-feature-projection/v2",
            Self::RainGateControl => "adjustment-rain-gate-control-projection/v3",
            Self::RainFixedGaugeProjection => "adjustment-rain-fixed-gauge-target-projection/v1",
            Self::TemperatureNativeSourceProjection => {
                "adjustment-temperature-native-source-projection/v2"
            }
            Self::RainControlState => "rain-maintenance-control-state/v1",
            Self::ShadowStageV1 => "adjustment-shadow-revision-stage/v1",
            Self::ShadowStageV2 => "adjustment-shadow-revision-stage/v2",
        }
    }
}

/// Source address of a control projection's supplementary monthly state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeRevisionGapRainBinding {
    pub state_sha256: RevisionGapSha256,
    pub ordinal_artifact_sha256: String,
    pub state_stage_receipt_sha256: String,
}

/// Fields extracted by the approved projection reader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeRevisionGapProjection {
    pub kind: NativeRevisionGapPayloadKind,
    pub rain_control: Option<NativeRevisionGapRainBinding>,
}

/// Source fields extracted by the canonical shadow-stage reader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeRevisionGapShadowStage {
    pub kind: NativeRevisionGapPayloadKind,
    pub due_key: String,
    pub registration_sha256: String,
    pub prediction_sha256: RevisionGapSha256,
    pub stage_receipt_bytes: Vec<u8>,
}

/// Exact supplementary canonical members before the base64/JSON encoder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeRevisionGapRainAuxiliary {
    pub state_bytes: Vec<u8>,
    pub stage_receipt_bytes: Vec<u8>,
    pub state_sha256: RevisionGapSha256,
    pub stage_receipt_member_sha256: RevisionGapSha256,
}

/// Actual file contents and member identities passed to the approved page encoder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeRevisionGapMember {
    pub kind: NativeRevisionGapPayloadKind,
    pub payload_bytes: Vec<u8>,
    pub payload_sha256: RevisionGapSha256,
    pub slot_sha256: RevisionGapSha256,
    pub gap_bytes: Vec<u8>,
    pub gap_member_sha256: RevisionGapSha256,
    pub stage_receipt_bytes: Vec<u8>,
    pub stage_receipt_member_sha256: RevisionGapSha256,
    pub rain_control: Option<NativeRevisionGapRainAuxiliary>,
}

/// Approved decoders and canonical codecs for actual native operator exports.
pub trait NativeRevisionGapTransferCodec:
    NativeRevisionSpoolArtifactCodec + RevisionGapIdentityAdapter<NativeRevisionGapMember>
{
    /// Decode the full canonical acknowledged frontier, including its nested identities.
    fn admitted_frontier(&self, bytes: &[u8]) -> Option<RevisionGapFrontier>;

    /// Admit a retained monthly state and optionally bind its ordinal for control projections.
    fn admitted_gap_rain_state(&self, bytes: &[u8], expected_ordinal: Option<&str>) -> bool {
        self.admitted_rain_control_ordinal_sha256(bytes)
            .is_some_and(|ordinal| expected_ordinal.is_none_or(|expected| expected == ordinal))
    }

    /// Decode and bind every original pending canonical member and page identity.
    fn admitted_pending(
        &self,
        bytes: &[u8],
    ) -> Option<RevisionGapPayloadPage<NativeRevisionGapMember>>;

    /// Encode exactly one selected canonical wire page including its registered identities.
    fn encode_page(
        &self,
        page: &RevisionGapPayloadPage<NativeRevisionGapMember>,
    ) -> Option<Vec<u8>>;

    /// Bind the complete canonical single commit receipt to its addressed frontier.
    fn single_commit_projection(
        &self,
        bytes: &[u8],
        frontier_sha256: RevisionGapSha256,
    ) -> Option<RevisionGapSha256>;

    /// Validate the complete canonical group and all derived receipt/chain identities.
    fn accepted_commit_group(&self, bytes: &[u8], projection_sha256: RevisionGapSha256) -> bool;

    /// Decode the staged payload class and optional exact rain-control identity fields.
    fn gap_projection(&self, bytes: &[u8]) -> Option<NativeRevisionGapProjection>;

    /// Validate the canonical stage receipt against the exact payload hash and projection kind.
    fn accepted_projection_stage(
        &self,
        bytes: &[u8],
        slot: RevisionGapSha256,
        kind: AdjustmentRevisionProjectionKind,
    ) -> bool;

    /// Decode the exact retained shadow document and its embedded stage receipt.
    fn gap_shadow_stage(&self, bytes: &[u8]) -> Option<NativeRevisionGapShadowStage>;
}

/// Exact local storage, content, and transfer-selection failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeRevisionGapTransferError {
    Storage,
    Census,
    Admission,
    Selection(RevisionGapTransferError),
}

type Outcome<T> = Result<T, NativeRevisionGapTransferError>;

/// One native page and the original canonical bytes frozen or replayed for its owner.
type NativePageOutput = (RevisionGapPayloadPage<NativeRevisionGapMember>, Vec<u8>);

impl From<RevisionGapTransferError> for NativeRevisionGapTransferError {
    /// Preserve the typed selection error below the command adapter.
    fn from(value: RevisionGapTransferError) -> Self {
        Self::Selection(value)
    }
}

impl<LeaseCodec: RevisionSpoolLeaseCodec> NativeRevisionSpool<LeaseCodec> {
    /// Observe the read-only frontier without creating directories or a pending page.
    pub fn read_gap_transfer_start(
        &self,
        codec: &mut impl NativeRevisionGapTransferCodec,
    ) -> Outcome<RevisionGapTransferStart> {
        self.require_gap_directories()?;
        read_revision_gap_transfer_start(self.read_gap_frontier(codec)?, codec).map_err(Into::into)
    }

    /// Return a retained page or freeze one new nonidle page under the shared native lease.
    pub fn read_gap_payload_page(
        &mut self,
        request: RevisionGapPageRequest,
        codec: &mut impl NativeRevisionGapTransferCodec,
    ) -> Outcome<RevisionGapPayloadPage<NativeRevisionGapMember>> {
        self.resolve_gap_payload_page(request, codec).map(
            // typed callers do not need to retain a second copy of the wire
            |(page, _)| page,
        )
    }

    /// Make the already encoded page available to the native command writer without re-encoding.
    pub(super) fn resolve_gap_payload_page(
        &mut self,
        request: RevisionGapPageRequest,
        codec: &mut impl NativeRevisionGapTransferCodec,
    ) -> Outcome<NativePageOutput> {
        self.require_gap_directories()?;
        self.with_lease(
            // preserve start and pending precedence before visiting staged members
            |storage| storage.read_gap_payload_page_locked(request, codec),
            NativeRevisionGapTransferError::Storage,
        )
    }

    /// Bind the deployed cursor directories without provisioning new ones during export.
    fn require_gap_directories(&self) -> Outcome<()> {
        require_directory(&self.root).map_err(storage_error)?;
        // require each exact fixed directory from the source owner's read path
        for name in GAP_TRANSFER_DIRS {
            require_directory(&self.root.join(name)).map_err(storage_error)?;
        }
        Ok(())
    }

    /// Read the exact current acknowledged state or project its digest-only genesis.
    fn read_gap_frontier(
        &self,
        codec: &impl NativeRevisionGapTransferCodec,
    ) -> Outcome<RevisionGapFrontier> {
        let path = self.root.join(TRANSFER).join("current.json");
        match read_optional_file(&path, REVISION_GAP_RECEIPT_MAXIMUM_BYTES)
            .map_err(storage_error)?
        {
            Some(bytes) => codec
                .admitted_frontier(&bytes)
                .ok_or(NativeRevisionGapTransferError::Admission),
            None => Ok(RevisionGapFrontier {
                frontier_sha256: RevisionGapSha256::admitted(
                    codec.sha256(b"adjustment-revision-gap-frontier/v1\n0\n"),
                ),
                page_count: 0,
            }),
        }
    }

    /// Return prior bytes first or collect and freeze the actual next value-bearing page.
    fn read_gap_payload_page_locked(
        &mut self,
        request: RevisionGapPageRequest,
        codec: &mut impl NativeRevisionGapTransferCodec,
    ) -> Outcome<NativePageOutput> {
        self.require_gap_directories()?;
        let frontier = self.read_gap_frontier(codec)?;
        let start = read_revision_gap_transfer_start(frontier, codec)?;
        // fail a changed cursor without reading pending bytes or collecting any values
        if start.start_sha256 != request.start_sha256
            || start.frontier_sha256 != request.frontier_sha256
        {
            return Err(RevisionGapTransferError::TransferStartDiffers.into());
        }
        let path = self.root.join(TRANSFER).join("pending.json");
        // return the original frozen bytes before visiting any newly staged files
        if let Some(bytes) =
            read_optional_file(&path, REVISION_GAP_PAGE_MAXIMUM_BYTES).map_err(storage_error)?
        {
            let page = codec
                .admitted_pending(&bytes)
                .ok_or(NativeRevisionGapTransferError::Admission)?;
            validate_retained_page(&page, request)?;
            // verify the exact pending bytes, not merely a semantically equivalent page
            if page.canonical_byte_count != bytes.len()
                || codec.encode_page(&page).as_deref() != Some(bytes.as_slice())
            {
                return Err(RevisionGapTransferError::RetainedPageDiffers.into());
            }
            return Ok((page, bytes));
        }
        let mut collector = GapCollector {
            storage: self,
            codec,
            census: RevisionGapRetainedCensus::default(),
        };
        let entries = collector.collect()?;
        let page = select_new_revision_gap_payload_page(request, collector.census, entries, codec)?;
        let bytes = codec
            .encode_page(&page)
            .ok_or(NativeRevisionGapTransferError::Admission)?;
        // keep the same measured envelope before writing or returning any bytes
        if bytes.len() != page.canonical_byte_count || bytes.len() > REVISION_GAP_PAGE_MAXIMUM_BYTES
        {
            return Err(RevisionGapTransferError::RetainedBytesDiffer.into());
        }
        // do not allocate a durable cursor for the encoded idle response
        if !page.idle {
            self.write_once(TRANSFER, "pending.json", &bytes)
                .map_err(|_| NativeRevisionGapTransferError::Storage)?;
        }
        Ok((page, bytes))
    }
}

/// One permanent metadata record with its original canonical bytes.
struct Gap {
    identity: RevisionGapSha256,
    bytes: Vec<u8>,
}

/// A projection-specific permanent disposition preserving the bound kind.
struct ProjectionGap {
    gap: Gap,
    kind: AdjustmentRevisionProjectionKind,
    body: Option<RevisionGapSha256>,
}

/// Native retained input after the payload's exact bytes have been checked.
struct EntryMaterial {
    kind: NativeRevisionGapPayloadKind,
    slot: RevisionGapSha256,
    payload_sha256: RevisionGapSha256,
    gap: Gap,
    bytes: Vec<u8>,
    receipt: Vec<u8>,
    rain: Option<NativeRevisionGapRainAuxiliary>,
    retained_bytes: RevisionGapRetainedBytes,
}

struct GapCollector<'a, Lease, Codec> {
    storage: &'a NativeRevisionSpool<Lease>,
    codec: &'a Codec,
    census: RevisionGapRetainedCensus,
}

impl<Lease: RevisionSpoolLeaseCodec, Codec: NativeRevisionGapTransferCodec>
    GapCollector<'_, Lease, Codec>
{
    /// Walk the deployed census groups in source order, keeping only complete selected payloads.
    fn collect(&mut self) -> Outcome<Vec<RetainedRevisionGapEntry<NativeRevisionGapMember>>> {
        let revision_gaps = self.projection_gaps()?;
        let shadow_gaps = self.shadow_gaps()?;
        let committed = self.committed_projection_set()?;
        let mut entries = Vec::new();
        self.collect_rain_states(&mut entries)?;
        self.collect_projections(&revision_gaps, &committed, &mut entries)?;
        self.collect_shadow(&shadow_gaps, &mut entries)?;
        Ok(entries)
    }

    /// Validate every durable revision gap and bind it to the existing filename preimage.
    fn projection_gaps(&mut self) -> Outcome<BTreeMap<RevisionGapSha256, Vec<ProjectionGap>>> {
        let files = self.names(REVISION_GAPS, REVISION_GAP_METADATA_MAXIMUM_FILES, false)?;
        self.census.revision_gap_files = files.len();
        let mut gaps: BTreeMap<_, Vec<_>> = BTreeMap::new();
        // admit all records even if a permanently classified body is no longer staged
        for name in files {
            let bytes = self.read(REVISION_GAPS, &name, REVISION_GAP_RECEIPT_MAXIMUM_BYTES)?;
            self.census.revision_gap_maximum_bytes =
                self.census.revision_gap_maximum_bytes.max(bytes.len());
            let receipt = self
                .codec
                .accepted_revision_gap(&bytes)
                .ok_or(NativeRevisionGapTransferError::Admission)?;
            self.bind_gap_name(&name, PermanentArchiveGapInput::Revision(&receipt.input))?;
            let (projection, body) = (
                nullable_digest(&receipt.input.projectionIdentitySha256)?,
                nullable_digest(&receipt.input.projectionSha256)?,
            );
            // retain only body-associated gaps after all canonical metadata has been observed
            if let Some(slot) = projection {
                gaps.entry(slot).or_default().push(ProjectionGap {
                    gap: Gap {
                        identity: decode_hex(&receipt.gap_sha256)?,
                        bytes,
                    },
                    kind: receipt.input.projectionKind,
                    body,
                });
            }
        }
        Ok(gaps)
    }

    /// Validate every durable shadow disposition by its exact scheduled registration key.
    fn shadow_gaps(&mut self) -> Outcome<BTreeMap<(String, String), Vec<Gap>>> {
        let files = self.names(SHADOW_GAPS, REVISION_GAP_METADATA_MAXIMUM_FILES, false)?;
        self.census.shadow_gap_files = files.len();
        let mut gaps: BTreeMap<_, Vec<_>> = BTreeMap::new();
        // retain the existing immutable first classifications for every scheduled key
        for name in files {
            let bytes = self.read(SHADOW_GAPS, &name, REVISION_GAP_RECEIPT_MAXIMUM_BYTES)?;
            self.census.shadow_gap_maximum_bytes =
                self.census.shadow_gap_maximum_bytes.max(bytes.len());
            let receipt = self
                .codec
                .accepted_shadow_gap(&bytes)
                .ok_or(NativeRevisionGapTransferError::Admission)?;
            self.bind_gap_name(&name, PermanentArchiveGapInput::Shadow(&receipt.input))?;
            gaps.entry((receipt.input.dueKey, receipt.input.registrationSha256))
                .or_default()
                .push(Gap {
                    identity: decode_hex(&receipt.gap_sha256)?,
                    bytes,
                });
        }
        Ok(gaps)
    }

    /// Exclude every actual committed slot, including complete grouped commit files.
    fn committed_projection_set(&mut self) -> Outcome<BTreeSet<RevisionGapSha256>> {
        let names = self.names(COMMITS, REVISION_GAP_ONLINE_MAXIMUM_FILES, true)?;
        let mut committed = BTreeSet::new();
        // preserve the separate single-then-grouped reading order
        for name in names.iter().filter(
            // visit legacy frontier names first
            |name| !name.starts_with("batch-"),
        ) {
            let bytes = self.read(COMMITS, name, REVISION_GAP_RECEIPT_MAXIMUM_BYTES)?;
            self.census.single_commit_receipt_files += 1;
            self.census.single_commit_receipt_maximum_bytes = self
                .census
                .single_commit_receipt_maximum_bytes
                .max(bytes.len());
            let projection = self
                .codec
                .single_commit_projection(&bytes, name_digest(name)?)
                .ok_or(NativeRevisionGapTransferError::Admission)?;
            committed.insert(projection);
        }
        // complete grouped commits occupy one admitted slot each
        for name in names.iter().filter(
            // use only the explicit batch-prefixed namespace
            |name| name.starts_with("batch-"),
        ) {
            let bytes = self.read(
                COMMITS,
                name,
                REVISION_GAP_BATCH_COMMIT_RECEIPT_MAXIMUM_BYTES,
            )?;
            self.census.batch_commit_receipt_files += 1;
            self.census.batch_commit_receipt_maximum_bytes = self
                .census
                .batch_commit_receipt_maximum_bytes
                .max(bytes.len());
            let projection = name_digest(&name[6..])?;
            // require the canonical file bound to its own grouped content address
            if !self.codec.accepted_commit_group(&bytes, projection) {
                return Err(NativeRevisionGapTransferError::Admission);
            }
            committed.insert(projection);
        }
        Ok(committed)
    }

    /// Collect exact abandoned state/receipt pairs by ascending retained filenames.
    fn collect_rain_states(
        &mut self,
        entries: &mut Vec<RetainedRevisionGapEntry<NativeRevisionGapMember>>,
    ) -> Outcome<()> {
        let mut names = self.names(RAIN_GAPS, REVISION_GAP_ONLINE_MAXIMUM_FILES, false)?;
        self.census.rain_control_state_gap_files = names.len();
        names.sort_unstable();
        // build each exact state pair before later projection or shadow entries
        for name in names {
            let slot = name_digest(&name)?;
            let state = self.rain_material(slot, None)?;
            let gap_bytes = self.read(RAIN_GAPS, &name, REVISION_GAP_RECEIPT_MAXIMUM_BYTES)?;
            let input = RainMaintenanceControlStateGapInput {
                reason: RainMaintenanceControlStateGapInputReasonLiteral,
                stageReceiptSha256: state.receipt_digest,
                stateSha256: lower_hex(*slot.as_bytes()),
            };
            let gap = decode_canonical_rain_gap_receipt(self.codec, &input, &gap_bytes)
                .map_err(|_| NativeRevisionGapTransferError::Admission)?;
            let retained = RevisionGapRetainedBytes::RainControlState {
                gap_bytes: gap_bytes.len(),
                payload_bytes: state.bytes.len(),
                stage_receipt_bytes: state.receipt_bytes.len(),
            };
            entries.push(self.entry(EntryMaterial {
                kind: NativeRevisionGapPayloadKind::RainControlState,
                slot,
                payload_sha256: slot,
                gap: Gap {
                    identity: decode_hex(&gap.gap_sha256)?,
                    bytes: gap_bytes,
                },
                bytes: state.bytes,
                receipt: state.receipt_bytes,
                rain: None,
                retained_bytes: retained,
            }));
        }
        Ok(())
    }

    /// Keep only uncommitted projection slots with one exact body-bound permanent gap.
    fn collect_projections(
        &mut self,
        gaps: &BTreeMap<RevisionGapSha256, Vec<ProjectionGap>>,
        committed: &BTreeSet<RevisionGapSha256>,
        entries: &mut Vec<RetainedRevisionGapEntry<NativeRevisionGapMember>>,
    ) -> Outcome<()> {
        let mut names = self.names(PROJECTIONS, REVISION_GAP_ONLINE_MAXIMUM_FILES, false)?;
        self.census.revision_projection_files = names.len();
        names.sort_unstable();
        // preserve source order and never read unchecked unclassified bodies
        for name in names {
            let slot = name_digest(&name)?;
            // skip a committed or still-in-flight payload before reading any value bytes
            if committed.contains(&slot) || !gaps.contains_key(&slot) {
                continue;
            }
            let [gap] = gaps[&slot].as_slice() else {
                return Err(NativeRevisionGapTransferError::Census);
            };
            let bytes = self.read(PROJECTIONS, &name, REVISION_GAP_PROJECTION_MAXIMUM_BYTES)?;
            let receipt = self.read(
                PROJECTION_RECEIPTS,
                &name,
                REVISION_GAP_RECEIPT_MAXIMUM_BYTES,
            )?;
            // require both source stage and permanent record to name these exact bytes
            if !self
                .codec
                .accepted_projection_stage(&receipt, slot, gap.kind)
                || gap.body != Some(slot)
                || self.digest(&bytes) != slot
            {
                return Err(NativeRevisionGapTransferError::Admission);
            }
            let projection = self
                .codec
                .gap_projection(&bytes)
                .ok_or(NativeRevisionGapTransferError::Admission)?;
            let rain = self.projection_auxiliary(&projection)?;
            let retained = RevisionGapRetainedBytes::Projection {
                gap_bytes: gap.gap.bytes.len(),
                payload_bytes: bytes.len(),
                stage_receipt_bytes: receipt.len(),
                rain_control_state: rain.as_ref().map(
                    // charge both native supplementary members
                    |state| RevisionGapRainControlStateBytes {
                        state_bytes: state.state_bytes.len(),
                        stage_receipt_bytes: state.stage_receipt_bytes.len(),
                    },
                ),
            };
            entries.push(self.entry(EntryMaterial {
                kind: projection.kind,
                slot,
                payload_sha256: slot,
                gap: Gap {
                    identity: gap.gap.identity,
                    bytes: gap.gap.bytes.clone(),
                },
                bytes,
                receipt,
                rain,
                retained_bytes: retained,
            }));
        }
        Ok(())
    }

    /// Retain exact scheduled shadow bytes only when permanently classified.
    fn collect_shadow(
        &mut self,
        gaps: &BTreeMap<(String, String), Vec<Gap>>,
        entries: &mut Vec<RetainedRevisionGapEntry<NativeRevisionGapMember>>,
    ) -> Outcome<()> {
        let mut names = self.names(SHADOW_STAGES, REVISION_GAP_ONLINE_MAXIMUM_FILES, false)?;
        self.census.shadow_stage_files = names.len();
        names.sort_unstable();
        // shadow requires decoding the stage before locating its scheduled key
        for name in names {
            let bytes = self.read(
                SHADOW_STAGES,
                &name,
                REVISION_GAP_SHADOW_STAGE_MAXIMUM_BYTES,
            )?;
            let stage = self
                .codec
                .gap_shadow_stage(&bytes)
                .ok_or(NativeRevisionGapTransferError::Admission)?;
            let key = (stage.due_key, stage.registration_sha256);
            let Some(found) = gaps.get(&key) else {
                continue;
            };
            let [gap] = found.as_slice() else {
                return Err(NativeRevisionGapTransferError::Census);
            };
            // tie the online slot to the exact decoded prediction and allowed retained stage
            if name_digest(&name)? != stage.prediction_sha256
                || !matches!(
                    stage.kind,
                    NativeRevisionGapPayloadKind::ShadowStageV1
                        | NativeRevisionGapPayloadKind::ShadowStageV2
                )
                || stage.stage_receipt_bytes.len() > REVISION_GAP_RECEIPT_MAXIMUM_BYTES
            {
                return Err(NativeRevisionGapTransferError::Admission);
            }
            let retained = RevisionGapRetainedBytes::ShadowStage {
                gap_bytes: gap.bytes.len(),
                stage_bytes: bytes.len(),
            };
            entries.push(self.entry(EntryMaterial {
                kind: stage.kind,
                slot: stage.prediction_sha256,
                payload_sha256: self.digest(&bytes),
                gap: Gap {
                    identity: gap.identity,
                    bytes: gap.bytes.clone(),
                },
                bytes,
                receipt: stage.stage_receipt_bytes,
                rain: None,
                retained_bytes: retained,
            }));
        }
        Ok(())
    }

    /// Read a complete controlled-rain pair only when the projection requires it.
    fn projection_auxiliary(
        &self,
        projection: &NativeRevisionGapProjection,
    ) -> Outcome<Option<NativeRevisionGapRainAuxiliary>> {
        use NativeRevisionGapPayloadKind::*;
        match (projection.kind, &projection.rain_control) {
            (RainGateControl, Some(binding)) => {
                let state = self.rain_material(binding.state_sha256, Some(binding))?;
                let stage_receipt_member_sha256 = self.digest(&state.receipt_bytes);
                Ok(Some(NativeRevisionGapRainAuxiliary {
                    state_bytes: state.bytes,
                    stage_receipt_bytes: state.receipt_bytes,
                    state_sha256: binding.state_sha256,
                    stage_receipt_member_sha256,
                }))
            }
            (Projection | BatchProjection | RainGateFeature | RainFixedGaugeProjection, None) => {
                Ok(None)
            }
            _ => Err(NativeRevisionGapTransferError::Admission),
        }
    }

    /// Verify a real monthly state's canonical format, hash, and exact first stage receipt.
    fn rain_material(
        &self,
        slot: RevisionGapSha256,
        binding: Option<&NativeRevisionGapRainBinding>,
    ) -> Outcome<RainMaterial> {
        let digest = lower_hex(*slot.as_bytes());
        let name = format!("sha256-{digest}.json");
        let bytes = self.read(
            RAIN_STATES,
            &name,
            REVISION_GAP_RAIN_CONTROL_STATE_MAXIMUM_BYTES,
        )?;
        let receipt_bytes = self.read(RAIN_RECEIPTS, &name, REVISION_GAP_RECEIPT_MAXIMUM_BYTES)?;
        let stage = decode_canonical_rain_stage_receipt(self.codec, &receipt_bytes, &digest)
            .map_err(|_| NativeRevisionGapTransferError::Admission)?;
        // retain the gap-specific state admission and bind detached control metadata only when supplied
        if self.digest(&bytes) != slot
            || !self.codec.admitted_gap_rain_state(
                &bytes,
                binding.map(|binding| binding.ordinal_artifact_sha256.as_str()),
            )
            || binding.is_some_and(|binding| {
                stage.stageReceiptSha256 != binding.state_stage_receipt_sha256
            })
        {
            return Err(NativeRevisionGapTransferError::Admission);
        }
        Ok(RainMaterial {
            bytes,
            receipt_bytes,
            receipt_digest: stage.stageReceiptSha256,
        })
    }

    /// Bind one native admitted entry and its exact canonical member identity metadata.
    fn entry(&self, material: EntryMaterial) -> RetainedRevisionGapEntry<NativeRevisionGapMember> {
        RetainedRevisionGapEntry {
            gap_sha256: material.gap.identity,
            retained_bytes: material.retained_bytes,
            value: NativeRevisionGapMember {
                kind: material.kind,
                payload_sha256: material.payload_sha256,
                slot_sha256: material.slot,
                payload_bytes: material.bytes,
                gap_member_sha256: self.digest(&material.gap.bytes),
                gap_bytes: material.gap.bytes,
                stage_receipt_member_sha256: self.digest(&material.receipt),
                stage_receipt_bytes: material.receipt,
                rain_control: material.rain,
            },
        }
    }

    /// Hash the actual member bytes exactly once for the approved wire encoder.
    fn digest(&self, bytes: &[u8]) -> RevisionGapSha256 {
        RevisionGapSha256::admitted(self.codec.sha256(bytes))
    }

    /// Require the exact lower-hex canonical filename derived from a verified metadata input.
    fn bind_gap_name(&self, name: &str, input: PermanentArchiveGapInput<'_>) -> Outcome<()> {
        input
            .validated_kind()
            .map_err(|_| NativeRevisionGapTransferError::Admission)?;
        let key = bounded_gap_bytes(self.codec.encode_gap_key(input))
            .map_err(|_| NativeRevisionGapTransferError::Admission)?;
        // compare the derived identity to the actual native file's canonical basename
        if self.digest(&key) != name_digest(name)? {
            return Err(NativeRevisionGapTransferError::Admission);
        }
        Ok(())
    }

    /// Observe one bounded native directory, admitting only the source filename grammars.
    fn names(&self, directory: &str, maximum: usize, allow_batch: bool) -> Outcome<Vec<String>> {
        let names = list_names(&self.storage.root.join(directory), Some(maximum + 1))
            .map_err(storage_error)?;
        // reject an overfull or unusable census before following any retained entry
        if names.len() > maximum
            || !names.iter().all(
                // grouped commits alone may have the fixed batch prefix
                |name| {
                    is_payload_name(name)
                        || (allow_batch && name.strip_prefix("batch-").is_some_and(is_payload_name))
                },
            )
        {
            return Err(NativeRevisionGapTransferError::Census);
        }
        Ok(names)
    }

    /// Open one exact bounded immutable member of a previously recognized directory.
    fn read(&self, directory: &str, name: &str, maximum: usize) -> Outcome<Vec<u8>> {
        read_private_file(&self.storage.root.join(directory).join(name), maximum)
            .map_err(storage_error)
    }
}

struct RainMaterial {
    bytes: Vec<u8>,
    receipt_bytes: Vec<u8>,
    receipt_digest: String,
}

/// Read one checked content identity from its already-recognized canonical name.
fn name_digest(name: &str) -> Outcome<RevisionGapSha256> {
    let text = name
        .strip_prefix("sha256-")
        .and_then(
            // require the exact native archive suffix
            |text| text.strip_suffix(".json"),
        )
        .ok_or(NativeRevisionGapTransferError::Census)?;
    decode_hex(text)
}

/// Convert generated nullable string coordinates without inventing a value for null.
fn nullable_digest(value: &Nullable<String>) -> Outcome<Option<RevisionGapSha256>> {
    match value {
        Nullable::Null => Ok(None),
        Nullable::Value(text) => decode_hex(text).map(Some),
    }
}

/// Decode a closed ASCII hex content identity, not a new hashing implementation.
fn decode_hex(text: &str) -> Outcome<RevisionGapSha256> {
    RevisionGapSha256::from_lower_hex(text).ok_or(NativeRevisionGapTransferError::Admission)
}

/// Redact operating-system details below the forced-command read adapter.
fn storage_error(_: std::io::Error) -> NativeRevisionGapTransferError {
    NativeRevisionGapTransferError::Storage
}
