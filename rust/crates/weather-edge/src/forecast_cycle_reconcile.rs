//! Pure warm-restart reconciliation for durable forecast cycle page frontiers.

use crate::forecast_cycle_pages::{
    FORECAST_CYCLE_MAXIMUM_PAGES, FORECAST_CYCLE_PAGE_ACK_CONTRACT_VERSION,
    FORECAST_CYCLE_PAGE_PAYLOAD_MAXIMUM_BYTES, ForecastCycleDurableAcknowledgement,
    ForecastCyclePage, ForecastCyclePageCodec, ForecastCyclePageSlot, ForecastCyclePageSlotKind,
    ForecastCyclePageState, ForecastCycleProjectionChannel, valid_state,
};
use crate::revision_archive_canonical::{RevisionArchiveCanonicalCodec, instant};

/// durable append-only classes recovered independently from online storage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForecastCycleDurableState<Gap, Finalized> {
    pub pages: Vec<ForecastCyclePage>,
    pub acknowledgements: Vec<ForecastCycleDurableAcknowledgement>,
    pub(crate) gaps: Vec<Gap>,
    pub(crate) finalized: Option<Finalized>,
}

impl ForecastCycleDurableState<(), ()> {
    /// Construct the open recovery input without installing opaque durable extensions.
    pub fn open(
        pages: Vec<ForecastCyclePage>,
        acknowledgements: Vec<ForecastCycleDurableAcknowledgement>,
    ) -> Self {
        Self {
            pages,
            acknowledgements,
            gaps: Vec::new(),
            finalized: None,
        }
    }
}

/// recovered cycle availability after authoritative durable replacement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastCycleReconcileStatus {
    Closed,
    Ready,
    AwaitingAcknowledgement,
}

impl ForecastCycleReconcileStatus {
    /// return the exact source status spelling.
    pub const fn wire(self) -> &'static str {
        match self {
            Self::Closed => "closed",
            Self::Ready => "ready",
            Self::AwaitingAcknowledgement => "awaiting_acknowledgement",
        }
    }
}

/// authoritative reconciled state and its immediately derived availability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForecastCycleReconcileResult<Gap, Finalized> {
    pub state: ForecastCyclePageState<Gap, Finalized>,
    pub status: ForecastCycleReconcileStatus,
}

/// closed warm-restart reconciliation failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastCycleReconcileError {
    InvalidExpectedState,
    InvalidDurableState,
    DurablePageCollision,
    HiddenPageSpoolRefused,
}

impl ForecastCycleReconcileError {
    /// return a source reason only for ordinary page refusals.
    pub const fn refusal_reason(self) -> Option<&'static str> {
        match self {
            Self::DurablePageCollision => Some("durable_page_collision"),
            Self::HiddenPageSpoolRefused => Some("hidden_page_spool_refused"),
            Self::InvalidExpectedState | Self::InvalidDurableState => None,
        }
    }
}

/// replace local projections with one validated durable prefix and at most two pending pages.
pub fn reconcile_forecast_cycle_pages<Gap, Finalized>(
    expected: &ForecastCyclePageState<Gap, Finalized>,
    durable: ForecastCycleDurableState<Gap, Finalized>,
    codec: &impl ForecastCyclePageCodec,
) -> Result<ForecastCycleReconcileResult<Gap, Finalized>, ForecastCycleReconcileError> {
    use ForecastCycleReconcileError as Error;

    // require the caller's known scope and frontier before comparing durable bytes
    if !valid_state(expected, codec) {
        return Err(Error::InvalidExpectedState);
    }
    if !valid_durable_pages(&durable.pages)
        || !valid_durable_acknowledgements(
            expected,
            &durable.pages,
            &durable.acknowledgements,
            codec,
        )
    {
        return Err(Error::InvalidDurableState);
    }

    // reject substitution only when neither complete typed page chain is a prefix
    if !is_prefix(&expected.pages, &durable.pages) && !is_prefix(&durable.pages, &expected.pages) {
        return Err(Error::DurablePageCollision);
    }
    let pending_count = durable.pages.len() - durable.acknowledgements.len();

    // retain only the source's current and next unacknowledged crash slots
    if pending_count > 2 {
        return Err(Error::HiddenPageSpoolRefused);
    }
    let slots = durable
        .pages
        .iter()
        .skip(durable.acknowledgements.len())
        .enumerate()
        .map(
            // rebuild physical slot order exclusively from the durable acknowledgement prefix
            |(index, page)| ForecastCyclePageSlot {
                page_index: page.page_index,
                page_sha256: page.page_sha256,
                slot: if index == 0 {
                    ForecastCyclePageSlotKind::Current
                } else {
                    ForecastCyclePageSlotKind::Next
                },
            },
        )
        .collect::<Vec<_>>();
    let (organic_projection_count, scheduler_projection_count) = count_channels(&durable.pages);
    let reconciled = ForecastCyclePageState {
        contract_version: expected.contract_version,
        due_key: expected.due_key.clone(),
        generation: expected.generation,
        local_date: expected.local_date.clone(),
        daily_page_count_at_open: expected.daily_page_count_at_open,
        pages: durable.pages,
        acknowledgements: durable.acknowledgements,
        gaps: durable.gaps,
        slots,
        organic_projection_count,
        scheduler_projection_count,
        finalized: durable.finalized,
    };

    // bind every durable acknowledgement and rebuilt slot through the shared exact validator
    if !valid_state(&reconciled, codec) {
        return Err(Error::InvalidDurableState);
    }
    let status = if reconciled.finalized.is_some() {
        ForecastCycleReconcileStatus::Closed
    } else if reconciled.slots.is_empty() {
        ForecastCycleReconcileStatus::Ready
    } else {
        ForecastCycleReconcileStatus::AwaitingAcknowledgement
    };
    Ok(ForecastCycleReconcileResult {
        state: reconciled,
        status,
    })
}

/// validate one consecutive durable page lineage before prefix comparison.
fn valid_durable_pages(pages: &[ForecastCyclePage]) -> bool {
    // retain the exact per-cycle page ceiling before traversing metadata vectors
    if pages.len() > FORECAST_CYCLE_MAXIMUM_PAGES {
        return false;
    }
    let mut predecessor = None;
    let mut payload_offset = 0_u64;

    // rebuild each page against only its already validated durable predecessor
    for (index, page) in pages.iter().enumerate() {
        if page.page_index != index
            || page.predecessor_page_sha256 != predecessor
            || page.payload_offset != payload_offset
            || page.payload_length == 0
            || page.payload_length > FORECAST_CYCLE_PAGE_PAYLOAD_MAXIMUM_BYTES
            || page.projection_identities.len() != page.projection_channels.len()
        {
            return false;
        }
        let Some(next_offset) = payload_offset.checked_add(page.payload_length as u64) else {
            return false;
        };
        payload_offset = next_offset;
        predecessor = Some(page.page_sha256);
    }
    true
}

/// validate one exact durable acknowledgement prefix before page collision checks.
fn valid_durable_acknowledgements<Gap, Finalized>(
    expected: &ForecastCyclePageState<Gap, Finalized>,
    pages: &[ForecastCyclePage],
    acknowledgements: &[ForecastCycleDurableAcknowledgement],
    codec: &impl ForecastCyclePageCodec,
) -> bool {
    // prohibit acknowledgements beyond the complete durable page chain
    if acknowledgements.len() > pages.len() {
        return false;
    }
    // bind every acknowledgement to its exact due scope, page position, clock, and digest
    for (index, durable) in acknowledgements.iter().enumerate() {
        let acknowledgement = &durable.acknowledgement;
        let Some(page) = pages.get(index) else {
            return false;
        };
        if acknowledgement.contract_version != FORECAST_CYCLE_PAGE_ACK_CONTRACT_VERSION
            || acknowledgement.due_key != expected.due_key
            || acknowledgement.generation != expected.generation
            || acknowledgement.page_index != index
            || acknowledgement.page_sha256 != page.page_sha256
            || instant(codec, &acknowledgement.acknowledged_at).is_none()
        {
            return false;
        }
        let Some(bytes) = codec.encode_page_acknowledgement(acknowledgement) else {
            return false;
        };
        if crate::forecast_cycle_pages::ForecastCycleSha256::admitted(
            RevisionArchiveCanonicalCodec::sha256(codec, &bytes),
        ) != durable.acknowledgement_sha256
        {
            return false;
        }
    }
    true
}

/// compare complete typed page records as one ordered prefix.
fn is_prefix(prefix: &[ForecastCyclePage], complete: &[ForecastCyclePage]) -> bool {
    prefix.len() <= complete.len()
        && prefix.iter().zip(complete).all(
            // preserve every canonical metadata field represented by the frozen typed page
            |(expected, durable)| expected == durable,
        )
}

/// recount both projection channel classes from durable page order.
fn count_channels(pages: &[ForecastCyclePage]) -> (usize, usize) {
    pages
        .iter()
        .flat_map(|page| page.projection_channels.iter())
        .fold(
            (0, 0),
            // increment only the exact closed channel class retained by each page
            |(organic, scheduler), channel| match channel {
                ForecastCycleProjectionChannel::Organic => (organic + 1, scheduler),
                ForecastCycleProjectionChannel::SchedulerRequest => (organic, scheduler + 1),
            },
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forecast_cycle_pages::{
        FORECAST_CYCLE_PAGE_ACK_CONTRACT_VERSION, ForecastCyclePageAcknowledgement,
        ForecastCyclePageStateInput, ForecastCycleSha256, create_forecast_cycle_page_state,
    };
    use crate::revision_archive_canonical::RevisionArchiveCanonicalCodec;

    const DUE_KEY: &str = "capture/2026-11-23T00:35:00.000Z";
    const ACKNOWLEDGED_AT: &str = "2026-11-23T00:36:00.000Z";

    struct FixtureCodec;

    impl RevisionArchiveCanonicalCodec for FixtureCodec {
        /// admit only the exact warm-restart acknowledgement fixture instant.
        fn canonical_utc_milliseconds(&self, text: &str) -> Option<i64> {
            (text == ACKNOWLEDGED_AT).then_some(1_795_394_160_000)
        }

        /// hash one typed fixture frame deterministically.
        fn sha256(&self, bytes: &[u8]) -> [u8; 32] {
            let mut digest = [u8::try_from(bytes.len() % 251).expect("bounded remainder"); 32];
            // retain enough source order to distinguish every fixture acknowledgement
            for (index, byte) in bytes.iter().enumerate() {
                let slot = index % digest.len();
                digest[slot] = digest[slot].wrapping_mul(31).wrapping_add(*byte);
            }
            digest
        }
    }

    impl ForecastCyclePageCodec for FixtureCodec {
        /// leave page-header encoding outside metadata-only reconciliation.
        fn encode_page_header(
            &self,
            _header: &crate::forecast_cycle_pages::ForecastCyclePageHeader,
        ) -> Option<Vec<u8>> {
            None
        }

        /// encode every retained acknowledgement identity into one fixture frame.
        fn encode_page_acknowledgement(
            &self,
            acknowledgement: &ForecastCyclePageAcknowledgement,
        ) -> Option<Vec<u8>> {
            Some(
                format!(
                    "{}|{}|{}|{}|{}|{}\n",
                    acknowledgement.contract_version,
                    acknowledgement.due_key,
                    acknowledgement.generation,
                    acknowledgement.page_index,
                    acknowledgement.page_sha256.as_bytes()[0],
                    acknowledgement.acknowledged_at,
                )
                .into_bytes(),
            )
        }

        /// leave two-segment payload hashing outside metadata-only reconciliation.
        fn sha256_pair(&self, _first: &[u8], _second: &[u8]) -> [u8; 32] {
            [0_u8; 32]
        }
    }

    /// construct one repeated digest fixture.
    fn digest(value: u8) -> ForecastCycleSha256 {
        ForecastCycleSha256::admitted([value; 32])
    }

    /// construct one exact empty expected cycle scope.
    fn expected() -> ForecastCyclePageState {
        create_forecast_cycle_page_state(ForecastCyclePageStateInput {
            daily_page_count: 0,
            due_key: DUE_KEY.to_owned(),
            generation: 7,
            local_date: "2026-11-23".to_owned(),
        })
        .expect("expected state")
    }

    /// construct one consecutive durable page fixture.
    fn page(
        index: usize,
        predecessor: Option<ForecastCycleSha256>,
        channel: ForecastCycleProjectionChannel,
    ) -> ForecastCyclePage {
        ForecastCyclePage {
            page_index: index,
            page_sha256: digest(10 + u8::try_from(index).expect("page index")),
            predecessor_page_sha256: predecessor,
            payload_sha256: digest(20 + u8::try_from(index).expect("page index")),
            payload_offset: u64::try_from(index).expect("page offset") * 5,
            payload_length: 5,
            projection_identities: vec![digest(
                30 + u8::try_from(index).expect("projection index"),
            )],
            projection_channels: vec![channel],
        }
    }

    /// construct one exact durable acknowledgement for a page prefix member.
    fn acknowledgement(
        page: &ForecastCyclePage,
        codec: &FixtureCodec,
    ) -> ForecastCycleDurableAcknowledgement {
        let acknowledgement = ForecastCyclePageAcknowledgement {
            contract_version: FORECAST_CYCLE_PAGE_ACK_CONTRACT_VERSION,
            due_key: DUE_KEY.to_owned(),
            generation: 7,
            page_index: page.page_index,
            page_sha256: page.page_sha256,
            acknowledged_at: ACKNOWLEDGED_AT.to_owned(),
        };
        let bytes = codec
            .encode_page_acknowledgement(&acknowledgement)
            .expect("ack bytes");
        ForecastCycleDurableAcknowledgement {
            acknowledgement,
            acknowledgement_sha256: ForecastCycleSha256::admitted(codec.sha256(&bytes)),
        }
    }

    /// construct one valid three-page durable lineage.
    fn pages() -> Vec<ForecastCyclePage> {
        let first = page(0, None, ForecastCycleProjectionChannel::Organic);
        let second = page(
            1,
            Some(first.page_sha256),
            ForecastCycleProjectionChannel::SchedulerRequest,
        );
        let third = page(
            2,
            Some(second.page_sha256),
            ForecastCycleProjectionChannel::Organic,
        );
        vec![first, second, third]
    }

    // verify one acknowledged prefix plus current and next survive warm restart
    #[test]
    fn recovers_mixed_acknowledged_current_and_next_frontier() {
        let codec = FixtureCodec;
        let pages = pages();
        let durable = ForecastCycleDurableState {
            acknowledgements: vec![acknowledgement(&pages[0], &codec)],
            finalized: None,
            gaps: Vec::new(),
            pages,
        };
        let reconciled =
            reconcile_forecast_cycle_pages(&expected(), durable, &codec).expect("reconciled");

        assert_eq!(
            reconciled.status,
            ForecastCycleReconcileStatus::AwaitingAcknowledgement
        );
        assert_eq!(reconciled.state.pages.len(), 3);
        assert_eq!(reconciled.state.acknowledgements.len(), 1);
        assert_eq!(
            reconciled
                .state
                .slots
                .iter()
                .map(|slot| (slot.page_index, slot.slot))
                .collect::<Vec<_>>(),
            vec![
                (1, ForecastCyclePageSlotKind::Current),
                (2, ForecastCyclePageSlotKind::Next),
            ]
        );
        assert_eq!(reconciled.state.organic_projection_count, 2);
        assert_eq!(reconciled.state.scheduler_projection_count, 1);
        assert_eq!(reconciled.state.due_key, DUE_KEY);
        assert_eq!(reconciled.state.generation, 7);
    }

    // verify internally broken and locally conflicting durable lineages remain distinct
    #[test]
    fn refuses_broken_and_conflicting_page_lineage() {
        let codec = FixtureCodec;
        let mut broken = pages();
        broken[1].predecessor_page_sha256 = Some(digest(99));
        assert_eq!(
            reconcile_forecast_cycle_pages(
                &expected(),
                ForecastCycleDurableState {
                    pages: broken,
                    acknowledgements: Vec::new(),
                    gaps: Vec::new(),
                    finalized: None,
                },
                &codec,
            ),
            Err(ForecastCycleReconcileError::InvalidDurableState)
        );

        let first = page(0, None, ForecastCycleProjectionChannel::Organic);
        let local = reconcile_forecast_cycle_pages(
            &expected(),
            ForecastCycleDurableState {
                pages: vec![first],
                acknowledgements: Vec::new(),
                gaps: Vec::new(),
                finalized: None,
            },
            &codec,
        )
        .expect("local state")
        .state;
        let mut replacement = page(0, None, ForecastCycleProjectionChannel::Organic);
        replacement.page_sha256 = digest(88);
        replacement.payload_sha256 = digest(89);
        assert_eq!(
            reconcile_forecast_cycle_pages(
                &local,
                ForecastCycleDurableState {
                    pages: vec![replacement],
                    acknowledgements: Vec::new(),
                    gaps: Vec::new(),
                    finalized: None,
                },
                &codec,
            ),
            Err(ForecastCycleReconcileError::DurablePageCollision)
        );
    }

    // verify a third unacknowledged durable page is never hidden outside fixed slots
    #[test]
    fn refuses_hidden_third_pending_page() {
        assert_eq!(
            reconcile_forecast_cycle_pages(
                &expected(),
                ForecastCycleDurableState {
                    pages: pages(),
                    acknowledgements: Vec::new(),
                    gaps: Vec::new(),
                    finalized: None,
                },
                &FixtureCodec,
            ),
            Err(ForecastCycleReconcileError::HiddenPageSpoolRefused)
        );
    }
}
