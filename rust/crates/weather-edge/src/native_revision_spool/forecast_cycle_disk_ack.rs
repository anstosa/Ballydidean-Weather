//! Durable cycle-page acknowledgement and fixed-slot promotion ordering.

use std::fs;
use std::io;
use std::path::Path;

use crate::forecast_cycle_pages::{
    ForecastCyclePageAckDurabilityProof, ForecastCyclePageAckPersistence,
    ForecastCyclePageAckPersistenceRequest, ForecastCyclePageCodec, ForecastCyclePageSlotKind,
    ForecastCycleSha256,
};
use crate::native_revision_spool_file::{publish_immutable, read_optional_file, sync_directory};

use super::forecast_allocation::{
    FORECAST_ALLOCATION_FREE_INODE_FLOOR, FORECAST_ALLOCATION_REQUIRED_FREE_BYTES,
    ForecastAllocationStatfsPort,
};
use super::forecast_cycle_disk_slots::{
    ForecastCycleDiskSlotCodec, ForecastCycleDiskSlotPage, NativeForecastCycleDiskSlots,
};

const ONLINE_DIRECTORY: &str = "online-pages";
const ACKNOWLEDGEMENT_DIRECTORY: &str = "acknowledgements";
const SLOT_DIRECTORY: &str = "slots";
const ACKNOWLEDGEMENT_MAXIMUM_BYTES: usize = 2_048;
const SAFE_JS_INTEGER_MAXIMUM: u64 = 9_007_199_254_740_991;

/// Existing validated current/next reader retained by acknowledgement persistence.
pub trait ForecastCycleDiskAckSlotPort {
    type Error;

    /// Borrow the admitted evidence root shared with the fixed slots.
    fn admitted_root(&self) -> &Path;

    /// Read one fully validated immutable current or next slot.
    fn read_disk_slot(
        &self,
        slot: ForecastCyclePageSlotKind,
    ) -> Result<Option<ForecastCycleDiskSlotPage>, Self::Error>;
}

impl<Codec: ForecastCycleDiskSlotCodec, Statfs> ForecastCycleDiskAckSlotPort
    for NativeForecastCycleDiskSlots<Codec, Statfs>
{
    type Error = super::forecast_cycle_disk_slots::ForecastCycleDiskSlotError;

    /// Reuse the root admitted by the slot initializer.
    fn admitted_root(&self) -> &Path {
        self.root()
    }

    /// Reuse exact slot framing, canonical decoding, and page validation.
    fn read_disk_slot(
        &self,
        slot: ForecastCyclePageSlotKind,
    ) -> Result<Option<ForecastCycleDiskSlotPage>, Self::Error> {
        self.read_slot(slot)
    }
}

/// Ordinary acknowledgement publication, collision, promotion, or capacity failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ForecastCycleDiskAckError {
    CapacityRefused,
    CapacityUnavailable,
    Codec,
    Collision,
    Slot,
    Storage,
    TooLarge,
}

/// Acknowledgement owner sharing one admitted slot reader with explicit codec and statfs ports.
pub struct NativeForecastCycleDiskAcknowledgements<Slots, Codec, Statfs> {
    slots: Slots,
    codec: Codec,
    statfs: Statfs,
}

impl<Slots, Codec, Statfs> NativeForecastCycleDiskAcknowledgements<Slots, Codec, Statfs> {
    /// Bind the same admitted slot owner to acknowledgement metadata and capacity owners.
    pub const fn new(slots: Slots, codec: Codec, statfs: Statfs) -> Self {
        Self {
            slots,
            codec,
            statfs,
        }
    }

    /// Borrow the shared slot owner after initialization.
    pub const fn slots(&self) -> &Slots {
        &self.slots
    }

    /// Mutably borrow the shared slot owner for later page persistence.
    pub fn slots_mut(&mut self) -> &mut Slots {
        &mut self.slots
    }

    /// Recover every injected owner after the acknowledgement lifecycle.
    pub fn into_parts(self) -> (Slots, Codec, Statfs) {
        (self.slots, self.codec, self.statfs)
    }
}

impl<Slots, Codec, Statfs> ForecastCyclePageAckPersistence
    for NativeForecastCycleDiskAcknowledgements<Slots, Codec, Statfs>
where
    Slots: ForecastCycleDiskAckSlotPort,
    Codec: ForecastCyclePageCodec,
    Statfs: ForecastAllocationStatfsPort,
{
    type Error = ForecastCycleDiskAckError;

    /// Publish the immutable receipt before unlinking or promoting either fixed slot.
    fn persist_acknowledgement(
        &mut self,
        request: ForecastCyclePageAckPersistenceRequest<'_>,
    ) -> Result<ForecastCyclePageAckDurabilityProof, Self::Error> {
        use ForecastCycleDiskAckError as Error;

        let bytes = self
            .codec
            .encode_page_acknowledgement(request.acknowledgement)
            .ok_or(Error::Codec)?;

        // bind the caller's immutable receipt address to the exact canonical bytes
        if bytes.len() > ACKNOWLEDGEMENT_MAXIMUM_BYTES
            || ForecastCycleSha256::admitted(self.codec.sha256(&bytes))
                != request.acknowledgement_sha256
        {
            return Err(if bytes.len() > ACKNOWLEDGEMENT_MAXIMUM_BYTES {
                Error::TooLarge
            } else {
                Error::Codec
            });
        }
        let root = self.slots.admitted_root().to_path_buf();
        admit(&mut self.statfs, &root, bytes.len())?;
        let acknowledgements = root.join(ONLINE_DIRECTORY).join(ACKNOWLEDGEMENT_DIRECTORY);
        let acknowledgement_path = acknowledgements.join(format!(
            "sha256-{}.json",
            crate::revision_archive_canonical::lower_hex(
                *request.acknowledgement_sha256.as_bytes(),
            )
        ));
        publish_or_verify(&acknowledgement_path, &bytes)?;
        let slots = root.join(ONLINE_DIRECTORY).join(SLOT_DIRECTORY);
        let current_path = slots.join("current.page");
        let next_path = slots.join("next.page");
        let current = self
            .slots
            .read_disk_slot(ForecastCyclePageSlotKind::Current)
            .map_err(|_| Error::Slot)?;

        // unlink only the exact current page bound by the durable acknowledgement
        if current
            .as_ref()
            .is_some_and(|page| page.page_sha256 == request.acknowledgement.page_sha256)
        {
            fs::remove_file(&current_path).map_err(|_| Error::Storage)?;
        } else if current.as_ref().is_some_and(|page| {
            page.header.predecessor_page_sha256 != Some(request.acknowledgement.page_sha256)
        }) {
            return Err(Error::Collision);
        }

        // promote only one already durable next slot after current becomes absent
        if request.next_slots.len() == 1
            && self
                .slots
                .read_disk_slot(ForecastCyclePageSlotKind::Current)
                .map_err(|_| Error::Slot)?
                .is_none()
        {
            match fs::rename(&next_path, &current_path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    let promoted = self
                        .slots
                        .read_disk_slot(ForecastCyclePageSlotKind::Current)
                        .map_err(|_| Error::Slot)?;

                    // accept only a retry after another owner already promoted current
                    if promoted.is_none() {
                        return Err(Error::Storage);
                    }
                }
                Err(_) => return Err(Error::Storage),
            }
        }
        sync_directory(&slots).map_err(|_| Error::Storage)?;
        Ok(ForecastCyclePageAckDurabilityProof {
            acknowledgement_sha256: request.acknowledgement_sha256,
            fsynced: true,
        })
    }
}

// publish one canonical acknowledgement or converge only its byte-identical retry
fn publish_or_verify(path: &Path, bytes: &[u8]) -> Result<(), ForecastCycleDiskAckError> {
    use ForecastCycleDiskAckError as Error;

    // preserve one already durable canonical receipt without rewriting it
    if let Some(existing) =
        read_optional_file(path, ACKNOWLEDGEMENT_MAXIMUM_BYTES).map_err(|_| Error::Storage)?
    {
        return if existing == bytes {
            Ok(())
        } else {
            Err(Error::Collision)
        };
    }
    let unique = super::uuid_from_linux_entropy().map_err(|_| Error::Storage)?;
    let temporary = path.with_file_name(format!(".capture-{unique}.tmp"));
    match publish_immutable(path, &temporary, bytes) {
        Ok(()) => Ok(()),
        Err(error)
            if error.source.kind() == io::ErrorKind::AlreadyExists
                && !error.temporary_cleanup_failed =>
        {
            let retained = read_optional_file(path, ACKNOWLEDGEMENT_MAXIMUM_BYTES)
                .map_err(|_| Error::Storage)?
                .ok_or(Error::Storage)?;

            // accept only an exact concurrent immutable receipt
            if retained == bytes {
                Ok(())
            } else {
                Err(Error::Collision)
            }
        }
        Err(_) => Err(Error::Storage),
    }
}

// apply the exact protected free-space and one-inode reservation before publication
fn admit(
    statfs: &mut impl ForecastAllocationStatfsPort,
    root: &Path,
    byte_length: usize,
) -> Result<(), ForecastCycleDiskAckError> {
    use ForecastCycleDiskAckError as Error;

    let sample = statfs
        .approved_sample(root)
        .map_err(|_| Error::CapacityUnavailable)?;

    // reject quantities outside the source JavaScript safe-integer boundary
    if sample.block_size == 0
        || sample.block_size > SAFE_JS_INTEGER_MAXIMUM
        || sample.free_inodes > SAFE_JS_INTEGER_MAXIMUM
    {
        return Err(Error::CapacityRefused);
    }
    let free_bytes = sample
        .available_blocks
        .checked_mul(sample.block_size)
        .ok_or(Error::CapacityRefused)?;
    let byte_length = u64::try_from(byte_length).map_err(|_| Error::CapacityRefused)?;
    let prospective_bytes = byte_length
        .checked_add(sample.block_size - 1)
        .and_then(|rounded| rounded.checked_div(sample.block_size))
        .and_then(|blocks| blocks.checked_mul(sample.block_size))
        .ok_or(Error::CapacityRefused)?;
    let required_free_bytes = FORECAST_ALLOCATION_REQUIRED_FREE_BYTES
        .checked_add(prospective_bytes)
        .ok_or(Error::CapacityRefused)?;
    let required_free_inodes = FORECAST_ALLOCATION_FREE_INODE_FLOOR
        .checked_add(1)
        .ok_or(Error::CapacityRefused)?;

    // preserve both post-write Node floors before the immutable receipt is created
    if free_bytes > SAFE_JS_INTEGER_MAXIMUM
        || free_bytes < required_free_bytes
        || sample.free_inodes < required_free_inodes
    {
        return Err(Error::CapacityRefused);
    }
    Ok(())
}

/// Admitted decoder for the exact canonical page-protocol acknowledgement record.
pub trait ForecastCycleDiskAckReadCodec: ForecastCyclePageCodec {
    /// Decode a complete canonical acknowledgement without changing its exact clock.
    fn decode_page_acknowledgement(
        &self,
        bytes: &[u8],
    ) -> Option<crate::forecast_cycle_pages::ForecastCyclePageAcknowledgement>;
}

impl<Codec: ForecastCycleDiskSlotCodec + ForecastCycleDiskAckReadCodec, Statfs>
    NativeForecastCycleDiskSlots<Codec, Statfs>
{
    /// Recover the exact durable page acknowledgement by its original page identity.
    pub fn read_cycle_page_acknowledgement(
        &self,
        page_sha256: ForecastCycleSha256,
    ) -> Result<
        Option<crate::forecast_cycle_pages::ForecastCycleDurableAcknowledgement>,
        ForecastCycleDiskAckError,
    > {
        use crate::forecast_cycle_pages::ForecastCycleDurableAcknowledgement;
        use crate::native_revision_spool_file::{list_names, read_private_file};
        use crate::revision_archive_canonical::{is_sha256, lower_hex};
        use ForecastCycleDiskAckError as Error;

        const MAXIMUM_ACKNOWLEDGEMENTS: usize = 8_192;
        let directory = self
            .root()
            .join(ONLINE_DIRECTORY)
            .join(ACKNOWLEDGEMENT_DIRECTORY);
        let mut names = list_names(&directory, Some(MAXIMUM_ACKNOWLEDGEMENTS + 1))
            .map_err(|_| Error::Storage)?;
        // reject an unbounded retained history without scanning its contents
        if names.len() > MAXIMUM_ACKNOWLEDGEMENTS {
            return Err(Error::TooLarge);
        }
        names.sort_unstable();
        // preserve Node's ordered scan and its first match
        for name in names {
            let expected = name
                .strip_prefix("sha256-")
                .and_then(|name| name.strip_suffix(".json"))
                .filter(|value| is_sha256(value))
                .ok_or(Error::Codec)?;
            let bytes = read_private_file(&directory.join(&name), ACKNOWLEDGEMENT_MAXIMUM_BYTES)
                .map_err(|_| Error::Storage)?;
            let digest = self.codec().sha256(&bytes);
            // bind the exact complete acknowledgement bytes to their content address
            if lower_hex(digest) != expected {
                return Err(Error::Collision);
            }
            let acknowledgement = self
                .codec()
                .decode_page_acknowledgement(&bytes)
                .ok_or(Error::Codec)?;
            // return only the exact page selected by the caller's transfer checkpoint
            if acknowledgement.page_sha256 == page_sha256 {
                return Ok(Some(ForecastCycleDurableAcknowledgement {
                    acknowledgement,
                    acknowledgement_sha256: ForecastCycleSha256::admitted(digest),
                }));
            }
        }
        Ok(None)
    }
}
