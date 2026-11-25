//! Native first-channel assertions for immutable forecast evidence receipts.

use std::io;
use std::path::{Path, PathBuf};

use super::forecast_objects::{FORECAST_RECEIPT_MAXIMUM_BYTES, ForecastObjectSha256};
use crate::native_revision_spool_file::{
    list_names, publish_immutable, read_optional_file, require_directory,
};
use crate::revision_archive_canonical::lower_hex;

/// maximum canonical bytes for one content-addressed channel assertion.
pub const FORECAST_CHANNEL_ASSERTION_MAXIMUM_BYTES: usize = 2_048;
/// maximum retained assertions and stable bindings without pruning.
pub const FORECAST_CHANNEL_MAXIMUM_ASSERTIONS: usize = 256;

const RECEIPT_DIRECTORY: &str = "receipts";
const ASSERTION_DIRECTORY: &str = "channels";
const BINDING_DIRECTORY: &str = "channel-bindings";

/// closed trusted issuance channels retained by the Node owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastIssuanceChannel {
    LegacyUnattributed,
    PublicGet,
    SchedulerRequest,
}

impl ForecastIssuanceChannel {
    /// return the exact wire spelling retained in a canonical assertion.
    pub const fn wire(self) -> &'static str {
        match self {
            Self::LegacyUnattributed => "legacy_unattributed",
            Self::PublicGet => "public_get",
            Self::SchedulerRequest => "scheduler_request",
        }
    }
}

/// minimal first-observed fields decoded from one exact retained receipt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedForecastChannelReceipt {
    pub first_edge_committed_at: String,
}

/// one assertion returned by the existing canonical codec and hash owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedForecastChannelAssertion {
    pub assertion_sha256: ForecastObjectSha256,
    pub bytes: Vec<u8>,
    pub channel: ForecastIssuanceChannel,
    pub edge_receipt_identity_sha256: ForecastObjectSha256,
    pub first_edge_committed_at: String,
    pub receipt_sha256: ForecastObjectSha256,
}

/// one stable receipt-to-assertion binding returned by the canonical codec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedForecastChannelBinding {
    pub assertion_sha256: ForecastObjectSha256,
    pub bytes: Vec<u8>,
    pub edge_receipt_identity_sha256: ForecastObjectSha256,
}

/// minimal fields decoded from one exact content-addressed assertion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedForecastChannelAssertion {
    pub channel: ForecastIssuanceChannel,
    pub edge_receipt_identity_sha256: ForecastObjectSha256,
    pub first_edge_committed_at: String,
    pub receipt_sha256: ForecastObjectSha256,
}

/// minimal content address decoded from one exact stable binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodedForecastChannelBinding {
    pub assertion_sha256: ForecastObjectSha256,
}

/// exact receipt, hash, assertion, and binding codecs retained outside storage.
pub trait ForecastChannelCodec {
    /// decode and validate one exact first receipt at its stable identity.
    fn decode_receipt(
        &self,
        expected_identity_sha256: ForecastObjectSha256,
        canonical_bytes: &[u8],
    ) -> Option<DecodedForecastChannelReceipt>;

    /// hash the exact retained first-receipt bytes.
    fn receipt_sha256(&self, canonical_bytes: &[u8]) -> ForecastObjectSha256;

    /// encode and hash one complete canonical channel assertion.
    fn encode_assertion(
        &self,
        channel: ForecastIssuanceChannel,
        edge_receipt_identity_sha256: ForecastObjectSha256,
        first_edge_committed_at: &str,
        receipt_sha256: ForecastObjectSha256,
    ) -> Option<EncodedForecastChannelAssertion>;

    /// decode and validate one assertion under its content address.
    fn decode_assertion(
        &self,
        expected_assertion_sha256: ForecastObjectSha256,
        canonical_bytes: &[u8],
    ) -> Option<DecodedForecastChannelAssertion>;

    /// encode one canonical stable binding for a receipt identity.
    fn encode_binding(
        &self,
        edge_receipt_identity_sha256: ForecastObjectSha256,
        assertion_sha256: ForecastObjectSha256,
    ) -> Option<EncodedForecastChannelBinding>;

    /// decode and validate one stable binding at its receipt identity.
    fn decode_binding(
        &self,
        expected_identity_sha256: ForecastObjectSha256,
        canonical_bytes: &[u8],
    ) -> Option<DecodedForecastChannelBinding>;
}

/// allocation facts passed to the authorized channel capacity owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForecastChannelAllocationRequest {
    pub assertion_bytes: usize,
    pub assertion_count: usize,
    pub assertion_exists: bool,
    pub assertion_sha256: ForecastObjectSha256,
    pub binding_bytes: usize,
    pub binding_count: usize,
    pub edge_receipt_identity_sha256: ForecastObjectSha256,
}

/// authorized channel capacity decision before immutable publication.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastChannelAllocationDecision {
    Admit,
    Refuse,
}

/// one assertion or stable binding publication observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForecastChannelPublication {
    pub bytes: usize,
    pub created: bool,
    pub identity_sha256: ForecastObjectSha256,
    pub kind: ForecastChannelPublicationKind,
}

/// closed immutable channel file kind reported to accounting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastChannelPublicationKind {
    Assertion,
    Binding,
}

/// authorized statfs, capacity, and channel accounting boundary.
pub trait ForecastChannelAllocationPort {
    type Error;

    /// admit or refuse one prospective assertion plus stable binding.
    fn admit(
        &mut self,
        request: ForecastChannelAllocationRequest,
    ) -> Result<ForecastChannelAllocationDecision, Self::Error>;

    /// observe each assertion and binding publication in source order.
    fn observe(&mut self, publication: ForecastChannelPublication);
}

/// one trusted channel request for an already-retained first receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForecastChannelCommitInput {
    pub edge_receipt_identity_sha256: ForecastObjectSha256,
    pub issuance_channel: Option<ForecastIssuanceChannel>,
    pub receipt_was_existing: bool,
}

/// completed or refused outcomes from the immutable channel slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastChannelCommitOutcome {
    Created,
    Existing,
    LegacyNoop,
    Refused(ForecastChannelCommitRefusal),
}

/// mutation-free channel refusal classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastChannelCommitRefusal {
    Allocation,
    Capacity,
    ObjectTooLarge,
}

/// native and binding failures distinct from ordinary outcomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastChannelCommitError {
    Accounting,
    AssertionCollision,
    BindingCollision,
    BindingPublication,
    Codec,
    ReceiptUnavailable,
    Storage,
}

/// concrete handle for existing receipt, assertion, and binding directories.
pub struct NativeForecastChannelStore {
    root: PathBuf,
}

impl NativeForecastChannelStore {
    /// admit one existing private root and its fixed channel children.
    pub fn new(root: PathBuf) -> io::Result<Self> {
        require_directory(&root)?;
        require_directory(&root.join(RECEIPT_DIRECTORY))?;
        require_directory(&root.join(ASSERTION_DIRECTORY))?;
        require_directory(&root.join(BINDING_DIRECTORY))?;
        Ok(Self { root })
    }

    /// borrow the admitted adjustment-evidence root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// preserve or create the first trusted issuance-channel binding.
    pub fn commit<Codec, Allocation>(
        &mut self,
        input: ForecastChannelCommitInput,
        codec: &Codec,
        allocation: &mut Allocation,
    ) -> Result<ForecastChannelCommitOutcome, ForecastChannelCommitError>
    where
        Codec: ForecastChannelCodec,
        Allocation: ForecastChannelAllocationPort,
    {
        // preserve legacy callers byte-identically without reading their receipt
        let Some(requested_channel) = input.issuance_channel else {
            return Ok(ForecastChannelCommitOutcome::LegacyNoop);
        };
        let receipt_bytes = read_optional_file(
            &self.receipt_path(input.edge_receipt_identity_sha256),
            FORECAST_RECEIPT_MAXIMUM_BYTES,
        )
        .map_err(|_| ForecastChannelCommitError::Storage)?
        .ok_or(ForecastChannelCommitError::ReceiptUnavailable)?;
        let receipt = codec
            .decode_receipt(input.edge_receipt_identity_sha256, &receipt_bytes)
            .ok_or(ForecastChannelCommitError::Codec)?;
        let receipt_sha256 = codec.receipt_sha256(&receipt_bytes);
        let binding_path = self.binding_path(input.edge_receipt_identity_sha256);
        let existing_binding = read_optional_file(&binding_path, FORECAST_RECEIPT_MAXIMUM_BYTES)
            .map_err(|_| ForecastChannelCommitError::Storage)?;
        // preserve the first channel already bound to this exact receipt
        if let Some(existing_binding) = existing_binding {
            self.validate_existing_binding(
                codec,
                input.edge_receipt_identity_sha256,
                &receipt.first_edge_committed_at,
                receipt_sha256,
                &existing_binding,
                true,
            )?;
            return Ok(ForecastChannelCommitOutcome::Existing);
        }

        let channel = if input.receipt_was_existing {
            ForecastIssuanceChannel::LegacyUnattributed
        } else {
            requested_channel
        };
        let assertion = codec
            .encode_assertion(
                channel,
                input.edge_receipt_identity_sha256,
                &receipt.first_edge_committed_at,
                receipt_sha256,
            )
            .ok_or(ForecastChannelCommitError::Codec)?;
        // bind every canonical assertion output to the exact requested receipt
        if assertion.channel != channel
            || assertion.edge_receipt_identity_sha256 != input.edge_receipt_identity_sha256
            || assertion.first_edge_committed_at != receipt.first_edge_committed_at
            || assertion.receipt_sha256 != receipt_sha256
        {
            return Err(ForecastChannelCommitError::Codec);
        }
        // keep the assertion inside its independent source ceiling
        if assertion.bytes.len() > FORECAST_CHANNEL_ASSERTION_MAXIMUM_BYTES {
            return Ok(ForecastChannelCommitOutcome::Refused(
                ForecastChannelCommitRefusal::ObjectTooLarge,
            ));
        }
        let binding = codec
            .encode_binding(
                input.edge_receipt_identity_sha256,
                assertion.assertion_sha256,
            )
            .ok_or(ForecastChannelCommitError::Codec)?;
        // bind stable binding bytes to the receipt and exact assertion address
        if binding.edge_receipt_identity_sha256 != input.edge_receipt_identity_sha256
            || binding.assertion_sha256 != assertion.assertion_sha256
        {
            return Err(ForecastChannelCommitError::Codec);
        }
        // keep the stable binding readable by the retained receipt-sized reader
        if binding.bytes.len() > FORECAST_RECEIPT_MAXIMUM_BYTES {
            return Ok(ForecastChannelCommitOutcome::Refused(
                ForecastChannelCommitRefusal::ObjectTooLarge,
            ));
        }

        let assertion_path = self.assertion_path(assertion.assertion_sha256);
        let assertion_exists =
            read_optional_file(&assertion_path, FORECAST_CHANNEL_ASSERTION_MAXIMUM_BYTES)
                .map_err(|_| ForecastChannelCommitError::Storage)?
                .is_some();
        let binding_count = list_names(
            &self.root.join(BINDING_DIRECTORY),
            Some(FORECAST_CHANNEL_MAXIMUM_ASSERTIONS + 1),
        )
        .map_err(|_| ForecastChannelCommitError::Storage)?
        .len();
        let assertion_count = list_names(
            &self.root.join(ASSERTION_DIRECTORY),
            Some(FORECAST_CHANNEL_MAXIMUM_ASSERTIONS + 1),
        )
        .map_err(|_| ForecastChannelCommitError::Storage)?
        .len();
        // preserve the fixed assertion and binding census without pruning
        if binding_count >= FORECAST_CHANNEL_MAXIMUM_ASSERTIONS
            || (!assertion_exists && assertion_count >= FORECAST_CHANNEL_MAXIMUM_ASSERTIONS)
        {
            return Ok(ForecastChannelCommitOutcome::Refused(
                ForecastChannelCommitRefusal::Capacity,
            ));
        }
        let allocation_request = ForecastChannelAllocationRequest {
            assertion_bytes: assertion.bytes.len(),
            assertion_count,
            assertion_exists,
            assertion_sha256: assertion.assertion_sha256,
            binding_bytes: binding.bytes.len(),
            binding_count,
            edge_receipt_identity_sha256: input.edge_receipt_identity_sha256,
        };
        // retain statfs and global allocation policy in its authorized owner
        if allocation
            .admit(allocation_request)
            .map_err(|_| ForecastChannelCommitError::Accounting)?
            == ForecastChannelAllocationDecision::Refuse
        {
            return Ok(ForecastChannelCommitOutcome::Refused(
                ForecastChannelCommitRefusal::Allocation,
            ));
        }

        let assertion_created = self.publish_or_verify_assertion(
            &assertion_path,
            &assertion.bytes,
            FORECAST_CHANNEL_ASSERTION_MAXIMUM_BYTES,
        )?;
        allocation.observe(ForecastChannelPublication {
            bytes: assertion.bytes.len(),
            created: assertion_created,
            identity_sha256: assertion.assertion_sha256,
            kind: ForecastChannelPublicationKind::Assertion,
        });

        let binding_created = self
            .publish_binding(&binding_path, &binding.bytes)
            .map_err(|_| ForecastChannelCommitError::BindingPublication)?;
        allocation.observe(ForecastChannelPublication {
            bytes: binding.bytes.len(),
            created: binding_created,
            identity_sha256: input.edge_receipt_identity_sha256,
            kind: ForecastChannelPublicationKind::Binding,
        });
        // converge a concurrent stable binding only through its exact assertion
        if !binding_created {
            let concurrent = read_optional_file(&binding_path, FORECAST_RECEIPT_MAXIMUM_BYTES)
                .map_err(|_| ForecastChannelCommitError::BindingPublication)?
                .ok_or(ForecastChannelCommitError::BindingPublication)?;
            self.validate_existing_binding(
                codec,
                input.edge_receipt_identity_sha256,
                &receipt.first_edge_committed_at,
                receipt_sha256,
                &concurrent,
                false,
            )
            .map_err(|error| match error {
                ForecastChannelCommitError::BindingCollision => error,
                _ => ForecastChannelCommitError::BindingPublication,
            })?;
            return Ok(ForecastChannelCommitOutcome::Existing);
        }
        Ok(ForecastChannelCommitOutcome::Created)
    }

    /// validate one stable binding and its exact content-addressed assertion.
    fn validate_existing_binding(
        &self,
        codec: &impl ForecastChannelCodec,
        edge_receipt_identity_sha256: ForecastObjectSha256,
        first_edge_committed_at: &str,
        receipt_sha256: ForecastObjectSha256,
        binding_bytes: &[u8],
        require_assertion_identity: bool,
    ) -> Result<(), ForecastChannelCommitError> {
        let binding = codec
            .decode_binding(edge_receipt_identity_sha256, binding_bytes)
            .ok_or(ForecastChannelCommitError::Codec)?;
        let assertion_bytes = read_optional_file(
            &self.assertion_path(binding.assertion_sha256),
            FORECAST_RECEIPT_MAXIMUM_BYTES,
        )
        .map_err(|_| ForecastChannelCommitError::Storage)?
        .ok_or(ForecastChannelCommitError::Storage)?;
        let assertion = codec
            .decode_assertion(binding.assertion_sha256, &assertion_bytes)
            .ok_or(ForecastChannelCommitError::Codec)?;
        // existing bindings additionally retain their exact receipt identity
        if (require_assertion_identity
            && assertion.edge_receipt_identity_sha256 != edge_receipt_identity_sha256)
            || assertion.receipt_sha256 != receipt_sha256
            || assertion.first_edge_committed_at != first_edge_committed_at
        {
            return Err(ForecastChannelCommitError::BindingCollision);
        }
        Ok(())
    }

    /// publish or verify one content-addressed canonical assertion.
    fn publish_or_verify_assertion(
        &self,
        target: &Path,
        bytes: &[u8],
        maximum_bytes: usize,
    ) -> Result<bool, ForecastChannelCommitError> {
        let temporary = self.temporary_path(target)?;
        match publish_immutable(target, &temporary, bytes) {
            Ok(()) => Ok(true),
            Err(error)
                if error.source.kind() == io::ErrorKind::AlreadyExists
                    && !error.temporary_cleanup_failed =>
            {
                let retained = read_optional_file(target, maximum_bytes)
                    .map_err(|_| ForecastChannelCommitError::Storage)?
                    .ok_or(ForecastChannelCommitError::Storage)?;
                if retained == bytes {
                    Ok(false)
                } else {
                    Err(ForecastChannelCommitError::AssertionCollision)
                }
            }
            Err(_) => Err(ForecastChannelCommitError::Storage),
        }
    }

    /// publish one stable binding without replacing a concurrent first writer.
    fn publish_binding(
        &self,
        target: &Path,
        bytes: &[u8],
    ) -> Result<bool, ForecastChannelCommitError> {
        let temporary = self.temporary_path(target)?;
        match publish_immutable(target, &temporary, bytes) {
            Ok(()) => Ok(true),
            Err(error)
                if error.source.kind() == io::ErrorKind::AlreadyExists
                    && !error.temporary_cleanup_failed =>
            {
                Ok(false)
            }
            Err(_) => Err(ForecastChannelCommitError::Storage),
        }
    }

    /// derive one unique source-shaped temporary beside its final target.
    fn temporary_path(&self, target: &Path) -> Result<PathBuf, ForecastChannelCommitError> {
        let directory = target.parent().ok_or(ForecastChannelCommitError::Storage)?;
        let unique =
            super::uuid_from_linux_entropy().map_err(|_| ForecastChannelCommitError::Storage)?;
        Ok(directory.join(format!(".capture-{unique}.tmp")))
    }

    /// derive the exact first-receipt path from its stable identity.
    fn receipt_path(&self, identity_sha256: ForecastObjectSha256) -> PathBuf {
        self.root.join(RECEIPT_DIRECTORY).join(format!(
            "sha256-{}.json",
            lower_hex(*identity_sha256.as_bytes())
        ))
    }

    /// derive the exact content-addressed assertion path.
    fn assertion_path(&self, assertion_sha256: ForecastObjectSha256) -> PathBuf {
        self.root.join(ASSERTION_DIRECTORY).join(format!(
            "sha256-{}.json",
            lower_hex(*assertion_sha256.as_bytes())
        ))
    }

    /// derive the exact stable receipt-to-assertion binding path.
    fn binding_path(&self, identity_sha256: ForecastObjectSha256) -> PathBuf {
        self.root.join(BINDING_DIRECTORY).join(format!(
            "sha256-{}.json",
            lower_hex(*identity_sha256.as_bytes())
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::fs::{self, DirBuilder, OpenOptions};
    use std::io::Write;
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    #[derive(Default)]
    struct Codec {
        assertions: Cell<usize>,
        bindings: Cell<usize>,
    }

    impl ForecastChannelCodec for Codec {
        /// decode one fixture receipt at its stable identity.
        fn decode_receipt(
            &self,
            expected: ForecastObjectSha256,
            bytes: &[u8],
        ) -> Option<DecodedForecastChannelReceipt> {
            (bytes.first().copied()? == expected.as_bytes()[0]).then_some(())?;
            Some(DecodedForecastChannelReceipt {
                first_edge_committed_at: std::str::from_utf8(bytes.get(1..)?).ok()?.to_owned(),
            })
        }

        /// hash exact fixture receipt bytes.
        fn receipt_sha256(&self, bytes: &[u8]) -> ForecastObjectSha256 {
            hash(bytes)
        }

        /// encode one closed fixture assertion.
        fn encode_assertion(
            &self,
            channel: ForecastIssuanceChannel,
            identity: ForecastObjectSha256,
            instant: &str,
            receipt_sha256: ForecastObjectSha256,
        ) -> Option<EncodedForecastChannelAssertion> {
            self.assertions.set(self.assertions.get() + 1);
            let mut bytes = vec![
                channel_byte(channel),
                identity.as_bytes()[0],
                receipt_sha256.as_bytes()[0],
            ];
            bytes.extend_from_slice(instant.as_bytes());
            Some(EncodedForecastChannelAssertion {
                assertion_sha256: hash(&bytes),
                bytes,
                channel,
                edge_receipt_identity_sha256: identity,
                first_edge_committed_at: instant.to_owned(),
                receipt_sha256,
            })
        }

        /// decode one content-addressed fixture assertion.
        fn decode_assertion(
            &self,
            expected: ForecastObjectSha256,
            bytes: &[u8],
        ) -> Option<DecodedForecastChannelAssertion> {
            (hash(bytes) == expected).then_some(())?;
            Some(DecodedForecastChannelAssertion {
                channel: channel(*bytes.first()?)?,
                edge_receipt_identity_sha256: digest(*bytes.get(1)?),
                first_edge_committed_at: std::str::from_utf8(bytes.get(3..)?).ok()?.to_owned(),
                receipt_sha256: digest(*bytes.get(2)?),
            })
        }

        /// encode one stable fixture binding.
        fn encode_binding(
            &self,
            identity: ForecastObjectSha256,
            assertion_sha256: ForecastObjectSha256,
        ) -> Option<EncodedForecastChannelBinding> {
            self.bindings.set(self.bindings.get() + 1);
            Some(EncodedForecastChannelBinding {
                assertion_sha256,
                bytes: vec![identity.as_bytes()[0], assertion_sha256.as_bytes()[0]],
                edge_receipt_identity_sha256: identity,
            })
        }

        /// decode one fixture binding at its stable identity.
        fn decode_binding(
            &self,
            expected: ForecastObjectSha256,
            bytes: &[u8],
        ) -> Option<DecodedForecastChannelBinding> {
            (bytes.len() == 2 && bytes[0] == expected.as_bytes()[0]).then_some(())?;
            Some(DecodedForecastChannelBinding {
                assertion_sha256: digest(bytes[1]),
            })
        }
    }

    struct Allocation {
        decision: ForecastChannelAllocationDecision,
        requests: Vec<ForecastChannelAllocationRequest>,
        observations: Vec<ForecastChannelPublication>,
    }

    impl Allocation {
        /// create one admitted allocation fixture.
        fn admitted() -> Self {
            Self {
                decision: ForecastChannelAllocationDecision::Admit,
                requests: Vec::new(),
                observations: Vec::new(),
            }
        }
    }

    impl ForecastChannelAllocationPort for Allocation {
        type Error = ();

        /// retain and return the selected allocation decision.
        fn admit(
            &mut self,
            request: ForecastChannelAllocationRequest,
        ) -> Result<ForecastChannelAllocationDecision, Self::Error> {
            self.requests.push(request);
            Ok(self.decision)
        }

        /// retain publication observations in source order.
        fn observe(&mut self, publication: ForecastChannelPublication) {
            self.observations.push(publication);
        }
    }

    struct Race {
        assertion: (PathBuf, Vec<u8>),
        binding: (PathBuf, Vec<u8>),
        observations: Vec<ForecastChannelPublication>,
    }

    impl ForecastChannelAllocationPort for Race {
        type Error = ();

        /// install one concurrent first assertion and binding.
        fn admit(
            &mut self,
            _request: ForecastChannelAllocationRequest,
        ) -> Result<ForecastChannelAllocationDecision, Self::Error> {
            write_private(&self.assertion.0, &self.assertion.1);
            write_private(&self.binding.0, &self.binding.1);
            Ok(ForecastChannelAllocationDecision::Admit)
        }

        /// retain local assertion and raced binding observations.
        fn observe(&mut self, publication: ForecastChannelPublication) {
            self.observations.push(publication);
        }
    }

    struct Root(PathBuf);

    impl Root {
        /// create one disposable owner-private channel tree.
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos();
            let root = std::env::temp_dir()
                .join(format!("weather-channels-{}-{nonce}", std::process::id()));
            private_directory(&root);
            // create only the three fixed channel fixture children
            for child in [RECEIPT_DIRECTORY, ASSERTION_DIRECTORY, BINDING_DIRECTORY] {
                private_directory(&root.join(child));
            }
            Self(root)
        }
    }

    impl Drop for Root {
        /// remove only the disposable channel tree.
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// create one exact owner-private directory.
    fn private_directory(path: &Path) {
        DirBuilder::new()
            .mode(0o700)
            .create(path)
            .expect("directory");
    }

    /// create one exact owner-private file.
    fn write_private(path: &Path, bytes: &[u8]) {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
            .expect("file");
        file.write_all(bytes).expect("bytes");
        file.sync_all().expect("sync");
    }

    /// construct one repeated digest fixture.
    fn digest(value: u8) -> ForecastObjectSha256 {
        ForecastObjectSha256::admitted([value; 32])
    }

    /// hash exact fixture bytes.
    fn hash(bytes: &[u8]) -> ForecastObjectSha256 {
        digest(
            bytes
                .iter()
                .fold(bytes.len() as u8, |value, byte| value ^ byte),
        )
    }

    /// encode one fixture issuance channel.
    fn channel_byte(channel: ForecastIssuanceChannel) -> u8 {
        match channel {
            ForecastIssuanceChannel::LegacyUnattributed => 1,
            ForecastIssuanceChannel::PublicGet => 2,
            ForecastIssuanceChannel::SchedulerRequest => 3,
        }
    }

    /// decode one fixture issuance channel.
    fn channel(value: u8) -> Option<ForecastIssuanceChannel> {
        match value {
            1 => Some(ForecastIssuanceChannel::LegacyUnattributed),
            2 => Some(ForecastIssuanceChannel::PublicGet),
            3 => Some(ForecastIssuanceChannel::SchedulerRequest),
            _ => None,
        }
    }

    /// retain one first receipt and return its exact bytes.
    fn receipt(
        store: &NativeForecastChannelStore,
        identity: ForecastObjectSha256,
        instant: &str,
    ) -> Vec<u8> {
        let mut bytes = vec![identity.as_bytes()[0]];
        bytes.extend_from_slice(instant.as_bytes());
        write_private(&store.receipt_path(identity), &bytes);
        bytes
    }

    /// construct one ordinary commit input.
    fn input(
        identity: ForecastObjectSha256,
        issuance_channel: Option<ForecastIssuanceChannel>,
        existing: bool,
    ) -> ForecastChannelCommitInput {
        ForecastChannelCommitInput {
            edge_receipt_identity_sha256: identity,
            issuance_channel,
            receipt_was_existing: existing,
        }
    }

    /// decode the assertion selected by one stable binding.
    fn selected_assertion(
        store: &NativeForecastChannelStore,
        codec: &Codec,
        identity: ForecastObjectSha256,
    ) -> DecodedForecastChannelAssertion {
        let binding_bytes = fs::read(store.binding_path(identity)).expect("binding");
        let binding = codec
            .decode_binding(identity, &binding_bytes)
            .expect("binding decode");
        let assertion_bytes =
            fs::read(store.assertion_path(binding.assertion_sha256)).expect("assertion");
        codec
            .decode_assertion(binding.assertion_sha256, &assertion_bytes)
            .expect("assertion decode")
    }

    // verify no-op, first channel, and retry behavior
    #[test]
    fn preserves_legacy_noop_and_first_public_binding() {
        let root = Root::new();
        let mut store = NativeForecastChannelStore::new(root.0.clone()).expect("store");
        let codec = Codec::default();
        let mut allocation = Allocation::admitted();
        let identity = digest(2);
        assert_eq!(
            store.commit(input(identity, None, false), &codec, &mut allocation),
            Ok(ForecastChannelCommitOutcome::LegacyNoop)
        );
        let receipt_bytes = receipt(&store, identity, "2026-11-23T12:00:00.000Z");
        assert_eq!(
            store.commit(
                input(identity, Some(ForecastIssuanceChannel::PublicGet), false),
                &codec,
                &mut allocation
            ),
            Ok(ForecastChannelCommitOutcome::Created)
        );
        let assertion = selected_assertion(&store, &codec, identity);
        assert_eq!(assertion.channel, ForecastIssuanceChannel::PublicGet);
        assert_eq!(assertion.receipt_sha256, hash(&receipt_bytes));
        assert_eq!(
            allocation
                .observations
                .iter()
                .map(|entry| entry.kind)
                .collect::<Vec<_>>(),
            vec![
                ForecastChannelPublicationKind::Assertion,
                ForecastChannelPublicationKind::Binding
            ]
        );
        assert_eq!(
            store.commit(
                input(
                    identity,
                    Some(ForecastIssuanceChannel::SchedulerRequest),
                    true
                ),
                &codec,
                &mut allocation
            ),
            Ok(ForecastChannelCommitOutcome::Existing)
        );
        assert_eq!(codec.assertions.get(), 1);
        assert_eq!(codec.bindings.get(), 1);
        assert_eq!(allocation.requests.len(), 1);
    }

    // verify first binding of a preexisting receipt stays unattributed
    #[test]
    fn selects_legacy_unattributed_for_preexisting_receipt() {
        let root = Root::new();
        let mut store = NativeForecastChannelStore::new(root.0.clone()).expect("store");
        let codec = Codec::default();
        let identity = digest(2);
        receipt(&store, identity, "2026-11-23T12:00:00.000Z");
        let mut allocation = Allocation::admitted();
        assert_eq!(
            store.commit(
                input(
                    identity,
                    Some(ForecastIssuanceChannel::SchedulerRequest),
                    true
                ),
                &codec,
                &mut allocation
            ),
            Ok(ForecastChannelCommitOutcome::Created)
        );
        assert_eq!(
            selected_assertion(&store, &codec, identity).channel,
            ForecastIssuanceChannel::LegacyUnattributed
        );
    }

    // verify a concurrent first binding remains authoritative
    #[test]
    fn converges_on_concurrent_binding_for_same_receipt() {
        let root = Root::new();
        let mut store = NativeForecastChannelStore::new(root.0.clone()).expect("store");
        let codec = Codec::default();
        let identity = digest(2);
        let instant = "2026-11-23T12:00:00.000Z";
        let receipt_bytes = receipt(&store, identity, instant);
        let assertion = codec
            .encode_assertion(
                ForecastIssuanceChannel::SchedulerRequest,
                identity,
                instant,
                hash(&receipt_bytes),
            )
            .expect("assertion");
        let binding = codec
            .encode_binding(identity, assertion.assertion_sha256)
            .expect("binding");
        let mut race = Race {
            assertion: (
                store.assertion_path(assertion.assertion_sha256),
                assertion.bytes,
            ),
            binding: (store.binding_path(identity), binding.bytes.clone()),
            observations: Vec::new(),
        };
        assert_eq!(
            store.commit(
                input(identity, Some(ForecastIssuanceChannel::PublicGet), false),
                &codec,
                &mut race
            ),
            Ok(ForecastChannelCommitOutcome::Existing)
        );
        assert_eq!(
            fs::read(store.binding_path(identity)).expect("stable binding"),
            binding.bytes
        );
        assert_eq!(race.observations.len(), 2);
        assert!(race.observations[0].created);
        assert!(!race.observations[1].created);
    }

    // verify refusal and existing-binding collision stay distinct
    #[test]
    fn distinguishes_allocation_refusal_and_binding_collision() {
        let root = Root::new();
        let mut store = NativeForecastChannelStore::new(root.0.clone()).expect("store");
        let codec = Codec::default();
        let identity = digest(2);
        receipt(&store, identity, "2026-11-23T12:00:00.000Z");
        let mut refused = Allocation {
            decision: ForecastChannelAllocationDecision::Refuse,
            ..Allocation::admitted()
        };
        assert_eq!(
            store.commit(
                input(identity, Some(ForecastIssuanceChannel::PublicGet), false),
                &codec,
                &mut refused
            ),
            Ok(ForecastChannelCommitOutcome::Refused(
                ForecastChannelCommitRefusal::Allocation
            ))
        );
        assert!(!store.binding_path(identity).exists());

        let wrong = codec
            .encode_assertion(
                ForecastIssuanceChannel::PublicGet,
                identity,
                "2026-11-23T11:00:00.000Z",
                digest(99),
            )
            .expect("wrong assertion");
        let binding = codec
            .encode_binding(identity, wrong.assertion_sha256)
            .expect("wrong binding");
        write_private(&store.assertion_path(wrong.assertion_sha256), &wrong.bytes);
        write_private(&store.binding_path(identity), &binding.bytes);
        let mut admitted = Allocation::admitted();
        assert_eq!(
            store.commit(
                input(identity, Some(ForecastIssuanceChannel::PublicGet), false),
                &codec,
                &mut admitted
            ),
            Err(ForecastChannelCommitError::BindingCollision)
        );
        assert!(admitted.requests.is_empty());
    }
}
