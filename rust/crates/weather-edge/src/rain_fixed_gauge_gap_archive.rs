//! Exact unit of custody for one immutable oversized rain target disposition.

use weather_contracts::{
    RainFixedGaugeTargetGapInput, RainFixedGaugeTargetGapInputReasonLiteral,
    RainFixedGaugeTargetGapReceipt, RainFixedGaugeTargetGapReceiptContractVersionLiteral,
    RainFixedGaugeTargetGapReceiptQualificationDispositionLiteral, RainFixedGaugeTargetGapStatus,
    RainFixedGaugeTargetGapStatusAbsent, RainFixedGaugeTargetGapStatusAbsentContractVersionLiteral,
    RainFixedGaugeTargetGapStatusAbsentStateLiteral, RainFixedGaugeTargetGapStatusInput,
    RainFixedGaugeTargetGapStatusPresent,
    RainFixedGaugeTargetGapStatusPresentContractVersionLiteral,
    RainFixedGaugeTargetGapStatusPresentGap,
    RainFixedGaugeTargetGapStatusPresentGapContractVersionLiteral,
    RainFixedGaugeTargetGapStatusPresentGapQualificationDispositionLiteral,
    RainFixedGaugeTargetGapStatusPresentGapReasonLiteral,
    RainFixedGaugeTargetGapStatusPresentStateLiteral,
};

use crate::private_archive_service::{PrivateArchiveFixedGaugeStore, PrivateArchiveStorageError};
use crate::revision_archive_canonical::{
    RevisionArchiveCanonicalClock, RevisionArchiveCanonicalCodec, instant, is_payload_name,
    is_sha256, lower_hex,
};

/// Exact private census directory within the admitted revision spool.
pub const RAIN_FIXED_GAUGE_GAP_DIRECTORY: &str = "rain-fixed-gauge-target-gaps";
const MAXIMUM_GAP_FILES: usize = 4_096;
const MAXIMUM_GAP_BYTES: usize = 4_096;
const HOUR_MILLISECONDS: i64 = 3_600_000;
const GAP_VERSION: &str = RainFixedGaugeTargetGapReceiptContractVersionLiteral::WIRE_VALUE;
const GAP_REASON: &str = RainFixedGaugeTargetGapInputReasonLiteral::WIRE_VALUE;
const DISPOSITION: &str = RainFixedGaugeTargetGapReceiptQualificationDispositionLiteral::WIRE_VALUE;

/// Ordinary typed archive errors kept behind the private relay's rejection response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RainFixedGaugeGapArchiveError {
    Input,
    StoredReceipt,
    Storage,
    Census,
    Clock,
}

/// One fixed-width content-addressed basename, derived only from a digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RainFixedGaugeGapFileName(String);

impl RainFixedGaugeGapFileName {
    /// Expose the exact basename for the owner-private directory adapter.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Closed codec boundary; the archive owns bytes and binding, not JSON parsing or crypto.
pub trait RainFixedGaugeGapCodec: RevisionArchiveCanonicalCodec {
    /// Decode the exact receipt DTO, including all literal fields.
    fn decode_receipt(&self, bytes: &[u8]) -> Option<RainFixedGaugeTargetGapReceipt>;
}

/// Native durability port for the existing owner-private revision spool.
pub trait RainFixedGaugeGapStorage {
    /// Verify the existing owner custody root and ensure the fixed private children.
    fn ensure_revision_root(&mut self) -> Result<(), RainFixedGaugeGapArchiveError>;

    /// Run under the existing cross-process revision spool mutation lease.
    fn with_revision_spool_mutation<T>(
        &mut self,
        mutation: impl FnOnce(&mut Self) -> Result<T, RainFixedGaugeGapArchiveError>,
    ) -> Result<T, RainFixedGaugeGapArchiveError>;

    /// Read one bounded regular private file without following links.
    fn read_gap_file(
        &mut self,
        name: &RainFixedGaugeGapFileName,
        maximum_bytes: usize,
    ) -> Result<Option<Vec<u8>>, RainFixedGaugeGapArchiveError>;

    /// Return at most the requested number of entries, stopping as soon as full.
    fn list_gap_names(
        &mut self,
        maximum_names: usize,
    ) -> Result<Vec<String>, RainFixedGaugeGapArchiveError>;

    /// Durably publish under an exclusive name, with data and parent sync.
    fn write_gap_exclusive(
        &mut self,
        name: &RainFixedGaugeGapFileName,
        bytes: &[u8],
    ) -> Result<(), RainFixedGaugeGapArchiveError>;
}

/// Real typed archive owner over the admitted storage, clock, and codec.
pub struct RainFixedGaugeGapArchive<Storage, Codec, Clock> {
    storage: Storage,
    codec: Codec,
    clock: Clock,
}

impl<Storage, Codec, Clock> RainFixedGaugeGapArchive<Storage, Codec, Clock>
where
    Storage: RainFixedGaugeGapStorage,
    Codec: RainFixedGaugeGapCodec,
    Clock: RevisionArchiveCanonicalClock,
{
    /// Bind one archive to the existing private spool and canonical codec.
    pub const fn new(storage: Storage, codec: Codec, clock: Clock) -> Self {
        Self {
            storage,
            codec,
            clock,
        }
    }

    /// Return the storage handle to its owning archive process.
    pub fn into_storage(self) -> Storage {
        self.storage
    }

    /// Record an exact disposition once, retaining the old timestamp on retries.
    pub fn record(
        &mut self,
        input: &RainFixedGaugeTargetGapInput,
    ) -> Result<RainFixedGaugeTargetGapReceipt, RainFixedGaugeGapArchiveError> {
        let key = GapKey::admit(&input.logicalHourAt, &input.logicalKeySha256, &self.codec)?;
        self.storage.ensure_revision_root()?;
        let name = key.filename(&self.codec);
        let codec = &self.codec;
        let clock = &mut self.clock;
        self.storage.with_revision_spool_mutation(
            // keep the read, census, and immutable publish under one shared lease
            |storage| {
                if let Some(bytes) = storage.read_gap_file(&name, MAXIMUM_GAP_BYTES)? {
                    return decode_bound_receipt(codec, &key, &bytes);
                }
                let names = storage.list_gap_names(MAXIMUM_GAP_FILES)?;
                // preserve retries at capacity while admitting no new gap beyond it
                if names.len() >= MAXIMUM_GAP_FILES
                    || !names.iter().map(String::as_str).all(is_payload_name)
                {
                    return Err(RainFixedGaugeGapArchiveError::Census);
                }
                let gap_at = clock
                    .canonical_now()
                    .ok_or(RainFixedGaugeGapArchiveError::Clock)?;
                // validate the canonical clock before embedding it in fixed JSON bytes
                if instant(codec, &gap_at).is_none() {
                    return Err(RainFixedGaugeGapArchiveError::Clock);
                }
                let unsigned = unsigned_receipt(&gap_at, &key);
                let digest = lower_hex(codec.sha256(unsigned.as_bytes()));
                let receipt = canonical_receipt(&gap_at, &digest, &key);
                storage.write_gap_exclusive(&name, receipt.as_bytes())?;
                Ok(RainFixedGaugeTargetGapReceipt {
                    logicalHourAt: key.hour.to_owned(),
                    logicalKeySha256: key.digest.to_owned(),
                    reason: RainFixedGaugeTargetGapInputReasonLiteral,
                    contractVersion: RainFixedGaugeTargetGapReceiptContractVersionLiteral,
                    gapAt: gap_at,
                    gapSha256: digest,
                    qualificationDisposition:
                        RainFixedGaugeTargetGapReceiptQualificationDispositionLiteral,
                })
            },
        )
    }

    /// Read exactly the bound gap or the canonical absent state without a mutation lease.
    pub fn status(
        &mut self,
        input: &RainFixedGaugeTargetGapStatusInput,
    ) -> Result<RainFixedGaugeTargetGapStatus, RainFixedGaugeGapArchiveError> {
        let key = GapKey::admit(&input.logicalHourAt, &input.logicalKeySha256, &self.codec)?;
        self.storage.ensure_revision_root()?;
        let name = key.filename(&self.codec);
        let Some(bytes) = self.storage.read_gap_file(&name, MAXIMUM_GAP_BYTES)? else {
            return Ok(RainFixedGaugeTargetGapStatus::Absent(
                RainFixedGaugeTargetGapStatusAbsent {
                    contractVersion: RainFixedGaugeTargetGapStatusAbsentContractVersionLiteral,
                    state: RainFixedGaugeTargetGapStatusAbsentStateLiteral,
                },
            ));
        };
        let gap = decode_bound_receipt(&self.codec, &key, &bytes)?;
        Ok(RainFixedGaugeTargetGapStatus::Present(
            RainFixedGaugeTargetGapStatusPresent {
                contractVersion: RainFixedGaugeTargetGapStatusPresentContractVersionLiteral,
                gap: RainFixedGaugeTargetGapStatusPresentGap {
                    contractVersion: RainFixedGaugeTargetGapStatusPresentGapContractVersionLiteral,
                    gapAt: gap.gapAt,
                    gapSha256: gap.gapSha256,
                    logicalHourAt: gap.logicalHourAt,
                    logicalKeySha256: gap.logicalKeySha256,
                    qualificationDisposition:
                        RainFixedGaugeTargetGapStatusPresentGapQualificationDispositionLiteral,
                    reason: RainFixedGaugeTargetGapStatusPresentGapReasonLiteral,
                },
                state: RainFixedGaugeTargetGapStatusPresentStateLiteral,
            },
        ))
    }
}

impl<Storage, Codec, Clock> PrivateArchiveFixedGaugeStore
    for RainFixedGaugeGapArchive<Storage, Codec, Clock>
where
    Storage: RainFixedGaugeGapStorage,
    Codec: RainFixedGaugeGapCodec,
    Clock: RevisionArchiveCanonicalClock,
{
    /// Map the fixed-gauge census to ordinary archive rejection rather than revision capacity.
    fn record_rain_fixed_gauge_target_gap(
        &mut self,
        input: RainFixedGaugeTargetGapInput,
    ) -> Result<RainFixedGaugeTargetGapReceipt, PrivateArchiveStorageError> {
        self.record(&input).map_err(
            // preserve the source's ordinary 409 disposition
            |_| PrivateArchiveStorageError::Rejected,
        )
    }

    /// Read one exact typed fixed-gauge status through the real archive owner.
    fn read_rain_fixed_gauge_target_gap(
        &mut self,
        input: RainFixedGaugeTargetGapStatusInput,
    ) -> Result<RainFixedGaugeTargetGapStatus, PrivateArchiveStorageError> {
        self.status(&input).map_err(
            // keep private file errors behind the normal archive rejection
            |_| PrivateArchiveStorageError::Rejected,
        )
    }
}

/// The exact two locator fields after canonical admission.
struct GapKey<'a> {
    hour: &'a str,
    digest: &'a str,
}

impl<'a> GapKey<'a> {
    /// Require the frozen UTC hour and lowercase digest grammar.
    fn admit(
        hour: &'a str,
        digest: &'a str,
        codec: &impl RainFixedGaugeGapCodec,
    ) -> Result<Self, RainFixedGaugeGapArchiveError> {
        let millis = instant(codec, hour).ok_or(RainFixedGaugeGapArchiveError::Input)?;
        // keep a fixed gauge target scoped to an exact UTC hour
        if millis % HOUR_MILLISECONDS != 0 || !is_sha256(digest) {
            return Err(RainFixedGaugeGapArchiveError::Input);
        }
        Ok(Self { hour, digest })
    }

    /// Address the file by the exact canonical key document.
    fn filename(&self, codec: &impl RainFixedGaugeGapCodec) -> RainFixedGaugeGapFileName {
        let bytes = format!(
            "{{\"logicalHourAt\":\"{}\",\"logicalKeySha256\":\"{}\"}}\n",
            self.hour, self.digest,
        );
        let hash = lower_hex(codec.sha256(bytes.as_bytes()));
        RainFixedGaugeGapFileName(format!("sha256-{hash}.json"))
    }
}

/// Rebind one retained receipt to the requested identity and canonical hash.
fn decode_bound_receipt(
    codec: &impl RainFixedGaugeGapCodec,
    key: &GapKey<'_>,
    bytes: &[u8],
) -> Result<RainFixedGaugeTargetGapReceipt, RainFixedGaugeGapArchiveError> {
    // enforce the file bound even if the native adapter misreports size
    if bytes.len() > MAXIMUM_GAP_BYTES {
        return Err(RainFixedGaugeGapArchiveError::StoredReceipt);
    }
    let receipt = codec
        .decode_receipt(bytes)
        .ok_or(RainFixedGaugeGapArchiveError::StoredReceipt)?;
    // validate all free fields before reserializing the exact durable body
    if instant(codec, &receipt.gapAt).is_none()
        || receipt.logicalHourAt != key.hour
        || receipt.logicalKeySha256 != key.digest
        || !is_sha256(&receipt.gapSha256)
    {
        return Err(RainFixedGaugeGapArchiveError::StoredReceipt);
    }
    let expected_hash = lower_hex(codec.sha256(unsigned_receipt(&receipt.gapAt, key).as_bytes()));
    // retain only the byte-identical canonical file and receipt identity
    if receipt.gapSha256 != expected_hash
        || bytes != canonical_receipt(&receipt.gapAt, &expected_hash, key).as_bytes()
    {
        return Err(RainFixedGaugeGapArchiveError::StoredReceipt);
    }
    Ok(receipt)
}

/// Encode the exact canonical preimage using only admitted fixed-alphabet fields.
fn unsigned_receipt(gap_at: &str, key: &GapKey<'_>) -> String {
    format!(
        "{{\"contractVersion\":\"{GAP_VERSION}\",\"gapAt\":\"{gap_at}\",\"logicalHourAt\":\"{}\",\"logicalKeySha256\":\"{}\",\"qualificationDisposition\":\"{DISPOSITION}\",\"reason\":\"{GAP_REASON}\"}}\n",
        key.hour, key.digest,
    )
}

/// Encode the exact newline-framed durable receipt in source key order.
fn canonical_receipt(gap_at: &str, digest: &str, key: &GapKey<'_>) -> String {
    format!(
        "{{\"contractVersion\":\"{GAP_VERSION}\",\"gapAt\":\"{gap_at}\",\"gapSha256\":\"{digest}\",\"logicalHourAt\":\"{}\",\"logicalKeySha256\":\"{}\",\"qualificationDisposition\":\"{DISPOSITION}\",\"reason\":\"{GAP_REASON}\"}}\n",
        key.hour, key.digest,
    )
}
