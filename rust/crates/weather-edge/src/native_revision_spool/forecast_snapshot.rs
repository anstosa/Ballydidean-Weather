//! Native startup validation and reusable frozen forecast-evidence catalogs.

use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::{Path, PathBuf};

use super::forecast_channels::ForecastChannelCodec;
use super::forecast_objects::{
    FORECAST_OBJECT_MAXIMUM_BYTES, FORECAST_RECEIPT_MAXIMUM_BYTES, ForecastObjectSha256,
};
use crate::RevisionGapSha256;
use crate::forecast_evidence_material::{
    RetainedForecastEvidenceObject, RetainedForecastEvidenceReceipt,
};
use crate::native_revision_spool_file::{list_names, read_private_file, require_directory};
use crate::revision_archive_canonical::{RevisionArchiveCanonicalClock, is_sha256, lower_hex};

/// exact portable snapshot contract retained by the Node export owner.
pub const FORECAST_EVIDENCE_SNAPSHOT_CONTRACT_VERSION: &str =
    "forecast-adjustment-evidence-snapshot/v1";

const OBJECT_DIRECTORY: &str = "objects";
const RECEIPT_DIRECTORY: &str = "receipts";
const CHANNEL_DIRECTORY: &str = "channels";
const CHANNEL_BINDING_DIRECTORY: &str = "channel-bindings";
const MAXIMUM_IDENTITIES: usize = 8_192;
const MAXIMUM_CHANNEL_ASSERTIONS: usize = 256;

/// portable digest pair supplied to the canonical watermark owner.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ForecastSnapshotWatermarkEntry {
    pub edge_receipt_identity_sha256: ForecastObjectSha256,
    pub object_sha256: ForecastObjectSha256,
}

/// gzip, JSON, hash, and watermark boundary over the existing channel codec.
pub trait ForecastSnapshotCodec: ForecastChannelCodec {
    /// validate one compressed canonical object under its content address.
    fn admit_retained_object(
        &self,
        expected_sha256: ForecastObjectSha256,
        compressed_bytes: &[u8],
    ) -> Option<RetainedForecastEvidenceObject>;

    /// validate one canonical first receipt under its stable identity.
    fn admit_retained_receipt(
        &self,
        expected_identity_sha256: ForecastObjectSha256,
        canonical_bytes: &[u8],
    ) -> Option<RetainedForecastEvidenceReceipt>;

    /// hash canonical portable watermark entries in their frozen order.
    fn watermark_sha256(
        &self,
        entries: &[ForecastSnapshotWatermarkEntry],
    ) -> Option<ForecastObjectSha256>;
}

/// one immutable catalog member with exact native paths and portable identities.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrozenForecastEvidenceEntry {
    pub edge_receipt_identity_sha256: ForecastObjectSha256,
    pub object_path: PathBuf,
    pub object_sha256: ForecastObjectSha256,
    pub receipt_path: PathBuf,
}

/// complete sorted catalog frozen from one startup receipt census.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrozenForecastEvidenceCatalog {
    entries: Vec<FrozenForecastEvidenceEntry>,
    frozen_at: String,
    watermark_sha256: ForecastObjectSha256,
}

impl FrozenForecastEvidenceCatalog {
    /// return the exact portable snapshot contract.
    pub const fn contract_version(&self) -> &'static str {
        FORECAST_EVIDENCE_SNAPSHOT_CONTRACT_VERSION
    }

    /// borrow entries in their frozen lexical receipt order.
    pub fn entries(&self) -> &[FrozenForecastEvidenceEntry] {
        &self.entries
    }

    /// borrow the canonical instant observed after successful validation.
    pub fn frozen_at(&self) -> &str {
        &self.frozen_at
    }

    /// return the portable ordered-entry watermark.
    pub const fn watermark_sha256(&self) -> ForecastObjectSha256 {
        self.watermark_sha256
    }
}

/// startup integrity or bounded native read failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeForecastSnapshotError {
    Capacity,
    ChannelInvalid,
    ChannelPairingInvalid,
    Clock,
    DirectoryInvalid,
    FilenameInvalid,
    ObjectInvalid,
    ReceiptInvalid,
    ReceiptPairingInvalid,
    Storage,
    Watermark,
}

impl Display for NativeForecastSnapshotError {
    /// render one bounded startup validation category.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        // retain distinct storage, object, receipt, channel, and pairing categories
        formatter.write_str(match self {
            Self::Capacity => "forecast evidence directory exceeds its count bound",
            Self::ChannelInvalid => "forecast evidence channel is invalid",
            Self::ChannelPairingInvalid => "forecast evidence channel pairing is invalid",
            Self::Clock => "forecast evidence snapshot clock is invalid",
            Self::DirectoryInvalid => "forecast evidence directory is invalid",
            Self::FilenameInvalid => "forecast evidence filename is invalid",
            Self::ObjectInvalid => "forecast evidence object is invalid",
            Self::ReceiptInvalid => "forecast evidence receipt is invalid",
            Self::ReceiptPairingInvalid => "forecast evidence receipt pairing is invalid",
            Self::Storage => "forecast evidence storage read failed",
            Self::Watermark => "forecast evidence snapshot watermark is invalid",
        })
    }
}

impl Error for NativeForecastSnapshotError {}

/// read-only native handle for startup validation and catalog freezing.
pub struct NativeForecastSnapshotCatalog {
    root: PathBuf,
}

impl NativeForecastSnapshotCatalog {
    /// bind one pre-existing owner-private evidence root and fixed children.
    pub fn new(root: PathBuf) -> Result<Self, NativeForecastSnapshotError> {
        require_directory(&root).map_err(|_| NativeForecastSnapshotError::DirectoryInvalid)?;

        // admit only the exact four retained directories
        for directory in [
            OBJECT_DIRECTORY,
            RECEIPT_DIRECTORY,
            CHANNEL_DIRECTORY,
            CHANNEL_BINDING_DIRECTORY,
        ] {
            require_directory(&root.join(directory))
                .map_err(|_| NativeForecastSnapshotError::DirectoryInvalid)?;
        }
        Ok(Self { root })
    }

    /// borrow the admitted evidence root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// exhaustively validate startup state and freeze the initial sorted receipt census.
    pub fn freeze<Codec, Clock>(
        &self,
        codec: &Codec,
        clock: &mut Clock,
    ) -> Result<FrozenForecastEvidenceCatalog, NativeForecastSnapshotError>
    where
        Codec: ForecastSnapshotCodec,
        Clock: RevisionArchiveCanonicalClock,
    {
        let names = self.census()?;
        self.validate_objects(&names.objects, codec)?;
        self.validate_channels(&names.channels, codec)?;
        self.validate_channel_bindings(&names.channel_bindings, codec)?;
        let entries = self.freeze_receipts(&names.receipts, codec)?;
        let watermark_entries = entries
            .iter()
            .map(|entry| ForecastSnapshotWatermarkEntry {
                edge_receipt_identity_sha256: entry.edge_receipt_identity_sha256,
                object_sha256: entry.object_sha256,
            })
            .collect::<Vec<_>>();
        let watermark_sha256 = codec
            .watermark_sha256(&watermark_entries)
            .ok_or(NativeForecastSnapshotError::Watermark)?;
        let frozen_at = clock
            .canonical_now()
            .ok_or(NativeForecastSnapshotError::Clock)?;
        Ok(FrozenForecastEvidenceCatalog {
            entries,
            frozen_at,
            watermark_sha256,
        })
    }

    /// freeze and validate the exact four directory name sets once.
    fn census(&self) -> Result<SnapshotNames, NativeForecastSnapshotError> {
        let objects = self.names(OBJECT_DIRECTORY, MAXIMUM_IDENTITIES, true)?;
        let mut receipts = self.names(RECEIPT_DIRECTORY, MAXIMUM_IDENTITIES, false)?;
        let channels = self.names(CHANNEL_DIRECTORY, MAXIMUM_CHANNEL_ASSERTIONS, false)?;
        let channel_bindings =
            self.names(CHANNEL_BINDING_DIRECTORY, MAXIMUM_CHANNEL_ASSERTIONS, false)?;
        receipts.sort();
        Ok(SnapshotNames {
            channel_bindings,
            channels,
            objects,
            receipts,
        })
    }

    /// list one complete bounded directory and validate every final name.
    fn names(
        &self,
        directory: &str,
        maximum: usize,
        compressed: bool,
    ) -> Result<Vec<String>, NativeForecastSnapshotError> {
        let names = list_names(&self.root.join(directory), Some(maximum + 1))
            .map_err(|_| NativeForecastSnapshotError::Storage)?;

        // reject overfull and unexpected immutable namespaces before reading bytes
        if names.len() > maximum {
            return Err(NativeForecastSnapshotError::Capacity);
        }

        // require every selected name to match its fixed content-addressed grammar
        if names
            .iter()
            .any(|name| digest_text_from_name(name, compressed).is_none())
        {
            return Err(NativeForecastSnapshotError::FilenameInvalid);
        }
        Ok(names)
    }

    /// validate every retained object, including receiptless crash-left objects.
    fn validate_objects<Codec: ForecastSnapshotCodec>(
        &self,
        names: &[String],
        codec: &Codec,
    ) -> Result<(), NativeForecastSnapshotError> {
        // validate every object selected by the startup census
        for name in names {
            let identity =
                digest_from_name(name, true).ok_or(NativeForecastSnapshotError::FilenameInvalid)?;
            let bytes = self.read(OBJECT_DIRECTORY, name, FORECAST_OBJECT_MAXIMUM_BYTES)?;
            codec
                .admit_retained_object(identity, &bytes)
                .ok_or(NativeForecastSnapshotError::ObjectInvalid)?;
        }
        Ok(())
    }

    /// validate every unbound or bound immutable channel assertion.
    fn validate_channels<Codec: ForecastSnapshotCodec>(
        &self,
        names: &[String],
        codec: &Codec,
    ) -> Result<(), NativeForecastSnapshotError> {
        // validate every assertion selected by the startup census
        for name in names {
            let identity = digest_from_name(name, false)
                .ok_or(NativeForecastSnapshotError::FilenameInvalid)?;
            let bytes = self.read(CHANNEL_DIRECTORY, name, FORECAST_RECEIPT_MAXIMUM_BYTES)?;
            codec
                .decode_assertion(identity, &bytes)
                .ok_or(NativeForecastSnapshotError::ChannelInvalid)?;
        }
        Ok(())
    }

    /// validate every stable binding against its assertion and first receipt.
    fn validate_channel_bindings<Codec: ForecastSnapshotCodec>(
        &self,
        names: &[String],
        codec: &Codec,
    ) -> Result<(), NativeForecastSnapshotError> {
        // preserve the complete frozen binding census
        for name in names {
            let expected_identity = digest_from_name(name, false)
                .ok_or(NativeForecastSnapshotError::FilenameInvalid)?;
            let binding_bytes = self.read(
                CHANNEL_BINDING_DIRECTORY,
                name,
                FORECAST_RECEIPT_MAXIMUM_BYTES,
            )?;
            let binding = codec
                .decode_binding(expected_identity, &binding_bytes)
                .ok_or(NativeForecastSnapshotError::ChannelInvalid)?;
            let assertion_name = digest_json_name(binding.assertion_sha256);
            let assertion_bytes = self.read(
                CHANNEL_DIRECTORY,
                &assertion_name,
                FORECAST_RECEIPT_MAXIMUM_BYTES,
            )?;
            let assertion = codec
                .decode_assertion(binding.assertion_sha256, &assertion_bytes)
                .ok_or(NativeForecastSnapshotError::ChannelInvalid)?;
            let receipt_name = digest_json_name(expected_identity);
            let receipt_bytes = self.read(
                RECEIPT_DIRECTORY,
                &receipt_name,
                FORECAST_RECEIPT_MAXIMUM_BYTES,
            )?;
            let receipt = codec
                .decode_receipt(expected_identity, &receipt_bytes)
                .ok_or(NativeForecastSnapshotError::ReceiptInvalid)?;

            // bind the pointer filename to the assertion and exact first receipt bytes
            if assertion.edge_receipt_identity_sha256 != expected_identity
                || assertion.receipt_sha256 != codec.receipt_sha256(&receipt_bytes)
                || assertion.first_edge_committed_at != receipt.first_edge_committed_at
            {
                return Err(NativeForecastSnapshotError::ChannelPairingInvalid);
            }
        }
        Ok(())
    }

    /// validate and freeze the initially selected sorted receipt list.
    fn freeze_receipts<Codec: ForecastSnapshotCodec>(
        &self,
        names: &[String],
        codec: &Codec,
    ) -> Result<Vec<FrozenForecastEvidenceEntry>, NativeForecastSnapshotError> {
        let mut entries = Vec::with_capacity(names.len());

        // never observe receipts added after the initial sorted census
        for name in names {
            let expected_identity = digest_from_name(name, false)
                .ok_or(NativeForecastSnapshotError::FilenameInvalid)?;
            let receipt_path = self.root.join(RECEIPT_DIRECTORY).join(name);
            let receipt_bytes = read_private_file(&receipt_path, FORECAST_RECEIPT_MAXIMUM_BYTES)
                .map_err(|_| NativeForecastSnapshotError::Storage)?;
            let receipt = codec
                .admit_retained_receipt(expected_identity, &receipt_bytes)
                .ok_or(NativeForecastSnapshotError::ReceiptInvalid)?;
            let object_sha256 = ForecastObjectSha256::admitted(receipt.object_sha256);
            let object_name = digest_object_name(object_sha256);
            let object_path = self.root.join(OBJECT_DIRECTORY).join(&object_name);
            let object_bytes = read_private_file(&object_path, FORECAST_OBJECT_MAXIMUM_BYTES)
                .map_err(|_| NativeForecastSnapshotError::Storage)?;
            let object = codec
                .admit_retained_object(object_sha256, &object_bytes)
                .ok_or(NativeForecastSnapshotError::ObjectInvalid)?;

            // bind availability, stable identity, window, and version to the object
            if receipt.availability_rows != object.row_count
                || object.stable_receipt_identity_sha256 != Some(*expected_identity.as_bytes())
                || receipt.window != object.window
                || receipt.version != object.version
            {
                return Err(NativeForecastSnapshotError::ReceiptPairingInvalid);
            }
            entries.push(FrozenForecastEvidenceEntry {
                edge_receipt_identity_sha256: expected_identity,
                object_path,
                object_sha256,
                receipt_path,
            });
        }
        Ok(entries)
    }

    /// read one fixed-directory private regular file within its source bound.
    fn read(
        &self,
        directory: &str,
        name: &str,
        maximum: usize,
    ) -> Result<Vec<u8>, NativeForecastSnapshotError> {
        read_private_file(&self.root.join(directory).join(name), maximum)
            .map_err(|_| NativeForecastSnapshotError::Storage)
    }
}

struct SnapshotNames {
    channel_bindings: Vec<String>,
    channels: Vec<String>,
    objects: Vec<String>,
    receipts: Vec<String>,
}

/// recover the lowercase digest member from one exact final basename.
fn digest_text_from_name(name: &str, compressed: bool) -> Option<&str> {
    let suffix = if compressed { ".json.gz" } else { ".json" };
    let digest = name.strip_prefix("sha256-")?.strip_suffix(suffix)?;
    is_sha256(digest).then_some(digest)
}

/// decode and cross-check one exact content-addressed basename.
fn digest_from_name(name: &str, compressed: bool) -> Option<ForecastObjectSha256> {
    let digest = digest_text_from_name(name, compressed)?;
    let decoded =
        ForecastObjectSha256::admitted(*RevisionGapSha256::from_lower_hex(digest)?.as_bytes());

    // require the injected decoder to preserve the exact lowercase identity
    if lower_hex(*decoded.as_bytes()) != digest {
        return None;
    }
    Some(decoded)
}

/// form one receipt, assertion, or binding filename from a typed digest.
fn digest_json_name(digest: ForecastObjectSha256) -> String {
    format!("sha256-{}.json", lower_hex(*digest.as_bytes()))
}

/// form one compressed object filename from a typed digest.
fn digest_object_name(digest: ForecastObjectSha256) -> String {
    format!("sha256-{}.json.gz", lower_hex(*digest.as_bytes()))
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::fs::{self, DirBuilder, OpenOptions};
    use std::io::Write;
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::super::forecast_channels::{
        DecodedForecastChannelAssertion, DecodedForecastChannelBinding,
        DecodedForecastChannelReceipt, EncodedForecastChannelAssertion,
        EncodedForecastChannelBinding, ForecastIssuanceChannel,
    };
    use super::*;
    use crate::{ForecastEvidenceObjectVersion, ForecastEvidenceWindow};

    const RECEIPT_A: ForecastObjectSha256 = ForecastObjectSha256::admitted([0x11; 32]);
    const RECEIPT_B: ForecastObjectSha256 = ForecastObjectSha256::admitted([0x22; 32]);
    const RECEIPT_A_HASH: ForecastObjectSha256 = ForecastObjectSha256::admitted([0x31; 32]);
    const OBJECT_A: ForecastObjectSha256 = ForecastObjectSha256::admitted([0xaa; 32]);
    const OBJECT_B: ForecastObjectSha256 = ForecastObjectSha256::admitted([0xbb; 32]);
    const ASSERTION_A: ForecastObjectSha256 = ForecastObjectSha256::admitted([0xcc; 32]);
    const ORPHAN: ForecastObjectSha256 = ForecastObjectSha256::admitted([0xdd; 32]);
    const WATERMARK: ForecastObjectSha256 = ForecastObjectSha256::admitted([0xee; 32]);
    const FIRST_COMMITTED_AT: &str = "2026-11-23T15:30:00.000Z";

    struct Fixture {
        path: PathBuf,
    }

    impl Fixture {
        /// create one exact four-directory private evidence root.
        fn new() -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("fixture clock")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "weather-forecast-snapshot-{}-{unique}",
                std::process::id()
            ));
            private_directory(&path);

            // create only the source's exact four fixed directories
            for directory in [
                OBJECT_DIRECTORY,
                RECEIPT_DIRECTORY,
                CHANNEL_DIRECTORY,
                CHANNEL_BINDING_DIRECTORY,
            ] {
                private_directory(&path.join(directory));
            }
            Self { path }
        }

        /// write one bounded private fixture member.
        fn write(&self, directory: &str, name: &str, bytes: &[u8]) {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(self.path.join(directory).join(name))
                .expect("create fixture member");
            file.write_all(bytes).expect("write fixture member");
        }

        /// populate two paired receipts, one channel, and one valid orphan object.
        fn populate(&self) {
            self.write(OBJECT_DIRECTORY, &digest_object_name(OBJECT_A), b"object-a");
            self.write(OBJECT_DIRECTORY, &digest_object_name(OBJECT_B), b"object-b");
            self.write(OBJECT_DIRECTORY, &digest_object_name(ORPHAN), b"orphan");
            // create receipts in reverse lexical order to prove explicit sorting
            self.write(
                RECEIPT_DIRECTORY,
                &digest_json_name(RECEIPT_B),
                b"receipt-b",
            );
            self.write(
                RECEIPT_DIRECTORY,
                &digest_json_name(RECEIPT_A),
                b"receipt-a",
            );
            self.write(
                CHANNEL_DIRECTORY,
                &digest_json_name(ASSERTION_A),
                b"assertion-a",
            );
            self.write(
                CHANNEL_BINDING_DIRECTORY,
                &digest_json_name(RECEIPT_A),
                b"binding-a",
            );
        }
    }

    impl Drop for Fixture {
        /// remove the bounded disposable native fixture.
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[derive(Default)]
    struct FixtureCodec {
        channel_mismatch: bool,
        watermark_entries: RefCell<Vec<ForecastSnapshotWatermarkEntry>>,
    }

    impl ForecastChannelCodec for FixtureCodec {
        /// decode one exact fixture receipt for channel pairing.
        fn decode_receipt(
            &self,
            expected: ForecastObjectSha256,
            bytes: &[u8],
        ) -> Option<DecodedForecastChannelReceipt> {
            (expected == RECEIPT_A && bytes == b"receipt-a").then(|| {
                DecodedForecastChannelReceipt {
                    first_edge_committed_at: FIRST_COMMITTED_AT.to_owned(),
                }
            })
        }

        /// hash one exact fixture receipt byte sequence.
        fn receipt_sha256(&self, bytes: &[u8]) -> ForecastObjectSha256 {
            // preserve the single channel-bound receipt fixture
            if bytes == b"receipt-a" {
                RECEIPT_A_HASH
            } else {
                ForecastObjectSha256::admitted([0; 32])
            }
        }

        /// keep channel writes outside this read-only snapshot fixture.
        fn encode_assertion(
            &self,
            _channel: ForecastIssuanceChannel,
            _edge_receipt_identity_sha256: ForecastObjectSha256,
            _first_edge_committed_at: &str,
            _receipt_sha256: ForecastObjectSha256,
        ) -> Option<EncodedForecastChannelAssertion> {
            None
        }

        /// decode one exact content-addressed channel assertion.
        fn decode_assertion(
            &self,
            expected: ForecastObjectSha256,
            bytes: &[u8],
        ) -> Option<DecodedForecastChannelAssertion> {
            (expected == ASSERTION_A && bytes == b"assertion-a").then(|| {
                DecodedForecastChannelAssertion {
                    channel: ForecastIssuanceChannel::PublicGet,
                    edge_receipt_identity_sha256: RECEIPT_A,
                    first_edge_committed_at: if self.channel_mismatch {
                        "2026-11-23T15:31:00.000Z".to_owned()
                    } else {
                        FIRST_COMMITTED_AT.to_owned()
                    },
                    receipt_sha256: RECEIPT_A_HASH,
                }
            })
        }

        /// keep binding writes outside this read-only snapshot fixture.
        fn encode_binding(
            &self,
            _edge_receipt_identity_sha256: ForecastObjectSha256,
            _assertion_sha256: ForecastObjectSha256,
        ) -> Option<EncodedForecastChannelBinding> {
            None
        }

        /// decode one exact stable fixture binding.
        fn decode_binding(
            &self,
            expected: ForecastObjectSha256,
            bytes: &[u8],
        ) -> Option<DecodedForecastChannelBinding> {
            (expected == RECEIPT_A && bytes == b"binding-a").then_some(
                DecodedForecastChannelBinding {
                    assertion_sha256: ASSERTION_A,
                },
            )
        }
    }

    impl ForecastSnapshotCodec for FixtureCodec {
        /// admit three exact canonical object fixtures.
        fn admit_retained_object(
            &self,
            expected: ForecastObjectSha256,
            bytes: &[u8],
        ) -> Option<RetainedForecastEvidenceObject> {
            // return only a byte-identical typed fixture object
            match (expected, bytes) {
                (OBJECT_A, b"object-a") => Some(RetainedForecastEvidenceObject {
                    row_count: 2,
                    stable_receipt_identity_sha256: Some(*RECEIPT_A.as_bytes()),
                    version: ForecastEvidenceObjectVersion::Legacy,
                    window: ForecastEvidenceWindow::OneDay,
                }),
                (OBJECT_B, b"object-b") => Some(RetainedForecastEvidenceObject {
                    row_count: 1,
                    stable_receipt_identity_sha256: Some(*RECEIPT_B.as_bytes()),
                    version: ForecastEvidenceObjectVersion::Extended,
                    window: ForecastEvidenceWindow::FiveDays,
                }),
                (ORPHAN, b"orphan") => Some(RetainedForecastEvidenceObject {
                    row_count: 1,
                    stable_receipt_identity_sha256: None,
                    version: ForecastEvidenceObjectVersion::Legacy,
                    window: ForecastEvidenceWindow::Overnight,
                }),
                _ => None,
            }
        }

        /// admit two exact first-receipt fixtures.
        fn admit_retained_receipt(
            &self,
            expected: ForecastObjectSha256,
            bytes: &[u8],
        ) -> Option<RetainedForecastEvidenceReceipt> {
            // return only a byte-identical typed fixture receipt
            match (expected, bytes) {
                (RECEIPT_A, b"receipt-a") => Some(RetainedForecastEvidenceReceipt {
                    availability_rows: 2,
                    first_edge_committed_at: FIRST_COMMITTED_AT.to_owned(),
                    object_sha256: *OBJECT_A.as_bytes(),
                    version: ForecastEvidenceObjectVersion::Legacy,
                    window: ForecastEvidenceWindow::OneDay,
                }),
                (RECEIPT_B, b"receipt-b") => Some(RetainedForecastEvidenceReceipt {
                    availability_rows: 1,
                    first_edge_committed_at: "2026-11-23T15:40:00.000Z".to_owned(),
                    object_sha256: *OBJECT_B.as_bytes(),
                    version: ForecastEvidenceObjectVersion::Extended,
                    window: ForecastEvidenceWindow::FiveDays,
                }),
                _ => None,
            }
        }

        /// retain the exact sorted portable digest pairs seen by the watermark owner.
        fn watermark_sha256(
            &self,
            entries: &[ForecastSnapshotWatermarkEntry],
        ) -> Option<ForecastObjectSha256> {
            self.watermark_entries.replace(entries.to_vec());
            Some(WATERMARK)
        }
    }

    struct FixtureClock;

    impl RevisionArchiveCanonicalClock for FixtureClock {
        /// return one already canonical snapshot clock observation.
        fn canonical_now(&mut self) -> Option<String> {
            Some("2026-11-23T16:00:00.000Z".to_owned())
        }
    }

    /// create one exact owner-private directory.
    fn private_directory(path: &Path) {
        let mut builder = DirBuilder::new();
        builder
            .mode(0o700)
            .create(path)
            .expect("create private fixture directory");
    }

    /// prove startup integrity, lexical receipt order, and portable frozen identity.
    #[test]
    fn validates_startup_and_freezes_sorted_receipts() {
        let fixture = Fixture::new();
        fixture.populate();
        let catalog =
            NativeForecastSnapshotCatalog::new(fixture.path.clone()).expect("admit fixture root");
        let codec = FixtureCodec::default();
        let frozen = catalog
            .freeze(&codec, &mut FixtureClock)
            .expect("freeze catalog");

        assert_eq!(
            frozen.contract_version(),
            "forecast-adjustment-evidence-snapshot/v1"
        );
        assert_eq!(frozen.frozen_at(), "2026-11-23T16:00:00.000Z");
        assert_eq!(frozen.watermark_sha256(), WATERMARK);
        assert_eq!(
            frozen
                .entries()
                .iter()
                .map(|entry| entry.edge_receipt_identity_sha256)
                .collect::<Vec<_>>(),
            [RECEIPT_A, RECEIPT_B]
        );
        assert_eq!(
            frozen
                .entries()
                .iter()
                .map(|entry| entry.object_sha256)
                .collect::<Vec<_>>(),
            [OBJECT_A, OBJECT_B]
        );
        assert_eq!(
            codec.watermark_entries.borrow().as_slice(),
            [
                ForecastSnapshotWatermarkEntry {
                    edge_receipt_identity_sha256: RECEIPT_A,
                    object_sha256: OBJECT_A,
                },
                ForecastSnapshotWatermarkEntry {
                    edge_receipt_identity_sha256: RECEIPT_B,
                    object_sha256: OBJECT_B,
                },
            ]
        );
        assert!(
            frozen.entries()[0]
                .object_path
                .ends_with(digest_object_name(OBJECT_A))
        );
        assert!(
            frozen.entries()[0]
                .receipt_path
                .ends_with(digest_json_name(RECEIPT_A))
        );
    }

    /// prove an ordinary channel mismatch stops startup catalog admission.
    #[test]
    fn rejects_a_mismatched_channel_binding() {
        let fixture = Fixture::new();
        fixture.populate();
        let catalog =
            NativeForecastSnapshotCatalog::new(fixture.path.clone()).expect("admit fixture root");
        let codec = FixtureCodec {
            channel_mismatch: true,
            ..FixtureCodec::default()
        };

        assert_eq!(
            catalog.freeze(&codec, &mut FixtureClock),
            Err(NativeForecastSnapshotError::ChannelPairingInvalid)
        );
    }

    /// prove valid receiptless objects remain part of exhaustive startup validation.
    #[test]
    fn rejects_a_corrupt_orphan_object() {
        let fixture = Fixture::new();
        fixture.populate();
        fs::write(
            fixture
                .path
                .join(OBJECT_DIRECTORY)
                .join(digest_object_name(ORPHAN)),
            b"corrupt",
        )
        .expect("replace orphan fixture bytes");
        let catalog =
            NativeForecastSnapshotCatalog::new(fixture.path.clone()).expect("admit fixture root");

        assert_eq!(
            catalog.freeze(&FixtureCodec::default(), &mut FixtureClock),
            Err(NativeForecastSnapshotError::ObjectInvalid)
        );
    }
}
