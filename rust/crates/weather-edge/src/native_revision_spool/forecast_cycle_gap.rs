//! Native immutable evidence gap records retain the first rejected projection.

use crate::forecast_cycle_pages::{ForecastCycleSha256, valid_due_key};
use crate::native_revision_spool_file::{list_names, publish_immutable, read_private_file};
use crate::revision_archive_canonical::{
    RevisionArchiveCanonicalCodec, instant, is_sha256, lower_hex,
};

use super::forecast_allocation::ForecastAllocationStatfsPort;
use super::forecast_cycle_disk_slots::{
    ForecastCycleDiskSlotCodec, ForecastCycleDiskSlotError, NativeForecastCycleDiskSlots,
};
use super::uuid_from_linux_entropy;

const GAP_DIRECTORY: &str = "online-pages/gaps";
const GAP_MAXIMUM_BYTES: usize = 2_048;
const GAP_MAXIMUM_RETAINED: usize = 8_192;

/// Source contract for a permanently unqualified projection.
pub const FORECAST_CYCLE_GAP_CONTRACT_VERSION: &str = "adjustment-evidence-gap/v1";
/// Permanent disposition retained on every evidence gap.
pub const FORECAST_CYCLE_GAP_DISPOSITION: &str = "forever_unqualified";

/// Closed categorical reasons for retaining an unmet evidence projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastCycleGapReason {
    ArchiveRefused,
    CapacityRefused,
    PageLimit,
    ProjectionLimit,
    SlotUnavailable,
    TransportRefused,
    Unacknowledged,
}

impl ForecastCycleGapReason {
    /// Emit only the exact source category.
    pub const fn wire(self) -> &'static str {
        match self {
            Self::ArchiveRefused => "archive_refused",
            Self::CapacityRefused => "capacity_refused",
            Self::PageLimit => "page_limit",
            Self::ProjectionLimit => "projection_limit",
            Self::SlotUnavailable => "slot_unavailable",
            Self::TransportRefused => "transport_refused",
            Self::Unacknowledged => "unacknowledged",
        }
    }

    /// Decode one closed category without accepting non-source aliases.
    pub fn from_wire(value: &str) -> Option<Self> {
        match value {
            "archive_refused" => Some(Self::ArchiveRefused),
            "capacity_refused" => Some(Self::CapacityRefused),
            "page_limit" => Some(Self::PageLimit),
            "projection_limit" => Some(Self::ProjectionLimit),
            "slot_unavailable" => Some(Self::SlotUnavailable),
            "transport_refused" => Some(Self::TransportRefused),
            "unacknowledged" => Some(Self::Unacknowledged),
            _ => None,
        }
    }
}

/// Fields retained beneath the exact existing gap contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForecastCycleGapRecord {
    pub due_key: String,
    pub generation: u64,
    pub projection_identity_sha256: ForecastCycleSha256,
    pub reason: ForecastCycleGapReason,
    pub occurred_at: String,
}

/// One validated retained content-addressed gap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableForecastCycleGap {
    record: ForecastCycleGapRecord,
    digest: ForecastCycleSha256,
}

impl DurableForecastCycleGap {
    /// Borrow the validated record without allowing later relabeling.
    pub const fn record(&self) -> &ForecastCycleGapRecord {
        &self.record
    }

    /// Return the digest verified against exact canonical storage bytes.
    pub const fn digest(&self) -> ForecastCycleSha256 {
        self.digest
    }
}

/// Exact wire encoding and decoding through the already admitted canonical owner.
pub trait ForecastCycleGapCodec: RevisionArchiveCanonicalCodec {
    /// Encode exact newline-framed canonical gap bytes.
    fn encode_cycle_gap(&self, record: &ForecastCycleGapRecord) -> Option<Vec<u8>>;

    /// Decode a complete canonical record and preserve its original field types.
    fn decode_cycle_gap(&self, bytes: &[u8]) -> Option<ForecastCycleGapRecord>;
}

/// Current write or first categorical gap already durable for the projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForecastCycleGapOutcome {
    Recorded(DurableForecastCycleGap),
    Retained(DurableForecastCycleGap),
}

/// Keep refused capacity separate from invalid retained custody and missing bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastCycleGapError {
    Capacity(ForecastCycleDiskSlotError),
    Codec,
    HistoryLimit,
    InvalidRecord,
    Storage,
}

impl<Codec, Statfs> NativeForecastCycleDiskSlots<Codec, Statfs>
where
    Codec: ForecastCycleDiskSlotCodec + ForecastCycleGapCodec,
    Statfs: ForecastAllocationStatfsPort,
{
    /// Read the finite canonical gap set for one exact cycle before reconstructing state.
    pub fn read_cycle_gaps(
        &self,
        due_key: &str,
        generation: u64,
    ) -> Result<Vec<DurableForecastCycleGap>, ForecastCycleGapError> {
        // refuse input outside the existing due-key grammar
        if !valid_due_key(due_key) {
            return Err(ForecastCycleGapError::InvalidRecord);
        }
        Ok(self
            .retained_cycle_gaps()?
            .into_iter()
            .filter(|gap| gap.record.due_key == due_key && gap.record.generation == generation)
            .collect())
    }

    /// Look up the first gap for a stable projection across all retained generations.
    pub fn has_cycle_gap(
        &self,
        due_key: &str,
        projection_identity_sha256: ForecastCycleSha256,
    ) -> Result<bool, ForecastCycleGapError> {
        // keep lookups in the existing due-key namespace
        if !valid_due_key(due_key) {
            return Err(ForecastCycleGapError::InvalidRecord);
        }
        Ok(self.retained_cycle_gaps()?.iter().any(|gap| {
            gap.record.due_key == due_key
                && gap.record.projection_identity_sha256 == projection_identity_sha256
        }))
    }

    /// Record one first categorical refusal under the caller's admitted archive lease.
    pub fn record_cycle_gap(
        &mut self,
        record: ForecastCycleGapRecord,
    ) -> Result<ForecastCycleGapOutcome, ForecastCycleGapError> {
        use ForecastCycleGapError as Error;
        use ForecastCycleGapOutcome as Outcome;

        // bind the due key and exact observed clock before attempting any durable write
        if !valid_due_key(&record.due_key) || instant(self.codec(), &record.occurred_at).is_none() {
            return Err(Error::InvalidRecord);
        }
        let retained = self.retained_cycle_gaps()?;
        // retain the first gap, even after a retry at a different occurrence clock
        if let Some(existing) = retained.iter().find(|gap| {
            gap.record.due_key == record.due_key
                && gap.record.projection_identity_sha256 == record.projection_identity_sha256
        }) {
            return Ok(Outcome::Retained(existing.clone()));
        }
        let bytes = self.codec().encode_cycle_gap(&record).ok_or(Error::Codec)?;
        // refuse an unbounded or non-roundtripping record before changing storage
        if bytes.len() > GAP_MAXIMUM_BYTES
            || self.codec().decode_cycle_gap(&bytes).as_ref() != Some(&record)
        {
            return Err(Error::InvalidRecord);
        }
        self.admit(bytes.len()).map_err(Error::Capacity)?;
        let digest = ForecastCycleSha256::admitted(self.codec().sha256(&bytes));
        let name = format!("sha256-{}.json", lower_hex(*digest.as_bytes()));
        let directory = self.root().join(GAP_DIRECTORY);
        let nonce = uuid_from_linux_entropy().map_err(|_| Error::Storage)?;
        let temporary = directory.join(format!(".capture-{nonce}.tmp"));
        publish_immutable(&directory.join(name), &temporary, &bytes).map_err(|_| Error::Storage)?;
        Ok(Outcome::Recorded(DurableForecastCycleGap {
            record,
            digest,
        }))
    }

    /// Validate every bounded gap in filename order before relying on any match.
    fn retained_cycle_gaps(&self) -> Result<Vec<DurableForecastCycleGap>, ForecastCycleGapError> {
        use ForecastCycleGapError as Error;

        let directory = self.root().join(GAP_DIRECTORY);
        let mut names =
            list_names(&directory, Some(GAP_MAXIMUM_RETAINED + 1)).map_err(|_| Error::Storage)?;
        // never silently use only a prefix of retained gap history
        if names.len() > GAP_MAXIMUM_RETAINED {
            return Err(Error::HistoryLimit);
        }
        names.sort_unstable();
        names
            .into_iter()
            .map(|name| {
                let expected = name
                    .strip_prefix("sha256-")
                    .and_then(|name| name.strip_suffix(".json"))
                    .filter(|value| is_sha256(value))
                    .ok_or(Error::InvalidRecord)?;
                let bytes = read_private_file(&directory.join(&name), GAP_MAXIMUM_BYTES)
                    .map_err(|_| Error::Storage)?;
                let record = self.codec().decode_cycle_gap(&bytes).ok_or(Error::Codec)?;
                let digest = self.codec().sha256(&bytes);
                // bind exact canonical bytes to both the address and typed owner
                if lower_hex(digest) != expected
                    || !valid_due_key(&record.due_key)
                    || instant(self.codec(), &record.occurred_at).is_none()
                {
                    return Err(Error::InvalidRecord);
                }
                Ok(DurableForecastCycleGap {
                    record,
                    digest: ForecastCycleSha256::admitted(digest),
                })
            })
            .collect()
    }
}
