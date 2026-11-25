//! Pure bounded WXACYCL2 containers for finalized forecast cycle payloads.

use crate::forecast_cycle_pages::{
    FORECAST_CYCLE_MAXIMUM_PAGES, FORECAST_CYCLE_PAGE_PAYLOAD_MAXIMUM_BYTES, ForecastCycleSha256,
};

use super::forecast_cycle_final_store::ForecastCycleFinalManifestCodec;

/// Exact persisted capsule contract returned after validation.
pub const FORECAST_CYCLE_CAPSULE_CONTRACT_VERSION: &str = "adjustment-cycle-capsule/v2";

const CAPSULE_MAGIC: [u8; 8] = *b"WXACYCL2";
const CAPSULE_HEADER_BYTES: usize = 16;
const MANIFEST_MAXIMUM_BYTES: usize = 32 * 1_024;
const CYCLE_MAXIMUM_PAYLOAD_BYTES: u64 = 4_832 * 1_024;

/// One ordered payload extent and digest admitted from a validated final manifest.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ForecastCycleCapsulePageRange {
    pub payload_offset: u64,
    pub payload_length: usize,
    pub payload_sha256: ForecastCycleSha256,
}

/// Complete bounded page geometry admitted from one final manifest.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ForecastCycleCapsuleManifestLayout {
    pub pages: Vec<ForecastCycleCapsulePageRange>,
    pub total_payload_bytes: u64,
}

/// Narrow projection from the existing validated manifest into capsule page ranges.
pub trait ForecastCycleCapsuleCodec: ForecastCycleFinalManifestCodec {
    /// Return exact ordered payload ranges only for an already validated final manifest.
    fn capsule_manifest_layout(
        &self,
        manifest: &Self::Manifest,
    ) -> Option<ForecastCycleCapsuleManifestLayout>;
}

/// One validated capsule with original manifest and page bytes borrowed from its container.
#[derive(Debug)]
pub struct ValidatedForecastCycleCapsule<'a, Manifest> {
    pub contract_version: &'static str,
    pub due_key: String,
    pub generation: u64,
    pub manifest: Manifest,
    pub manifest_bytes: &'a [u8],
    pub manifest_sha256: ForecastCycleSha256,
    pub page_payloads: Vec<&'a [u8]>,
    pub total_payload_bytes: u64,
}

/// Bounded header, manifest, page geometry, digest, and trailing-data failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ForecastCycleCapsuleError {
    Codec,
    Header,
    Manifest,
    ManifestIdentity,
    PageCount,
    PagePayload,
    TooLarge,
    TrailingBytes,
    Truncated,
}

/// Encode one exact finalized manifest followed by its verified ordered page payloads.
pub fn encode_forecast_cycle_capsule<Codec: ForecastCycleCapsuleCodec>(
    manifest: &Codec::Manifest,
    manifest_sha256: ForecastCycleSha256,
    page_payloads: &[&[u8]],
    codec: &Codec,
) -> Result<Vec<u8>, ForecastCycleCapsuleError> {
    use ForecastCycleCapsuleError as Error;

    let manifest_bytes = codec.encode_final_manifest(manifest).ok_or(Error::Codec)?;
    // bind the supplied immutable manifest identity before copying any page bytes
    if manifest_bytes.is_empty()
        || manifest_bytes.len() > MANIFEST_MAXIMUM_BYTES
        || codec.sha256(&manifest_bytes) != *manifest_sha256.as_bytes()
    {
        return Err(Error::ManifestIdentity);
    }
    let layout = codec
        .capsule_manifest_layout(manifest)
        .ok_or(Error::Manifest)?;
    validate_layout(&layout)?;
    // require one source payload for every final-manifest range
    if page_payloads.len() != layout.pages.len() {
        return Err(Error::PageCount);
    }
    // verify every payload length and digest before allocating the complete capsule
    for (payload, page) in page_payloads.iter().zip(&layout.pages) {
        if payload.len() != page.payload_length
            || codec.sha256(payload) != *page.payload_sha256.as_bytes()
        {
            return Err(Error::PagePayload);
        }
    }
    let manifest_length: u32 = manifest_bytes
        .len()
        .try_into()
        .map_err(|_| Error::TooLarge)?;
    let page_count: u32 = page_payloads
        .len()
        .try_into()
        .map_err(|_| Error::TooLarge)?;
    let payload_length: usize = layout
        .total_payload_bytes
        .try_into()
        .map_err(|_| Error::TooLarge)?;
    let capsule_length = CAPSULE_HEADER_BYTES
        .checked_add(manifest_bytes.len())
        .and_then(|length| length.checked_add(payload_length))
        .ok_or(Error::TooLarge)?;
    let mut capsule = Vec::with_capacity(capsule_length);
    capsule.extend_from_slice(&CAPSULE_MAGIC);
    capsule.extend_from_slice(&manifest_length.to_be_bytes());
    capsule.extend_from_slice(&page_count.to_be_bytes());
    capsule.extend_from_slice(&manifest_bytes);
    // preserve original page order without adding per-page framing
    for payload in page_payloads {
        capsule.extend_from_slice(payload);
    }
    Ok(capsule)
}

/// Validate one complete capsule and borrow its exact retained manifest and page byte ranges.
pub fn validate_forecast_cycle_capsule<'a, Codec: ForecastCycleCapsuleCodec>(
    capsule: &'a [u8],
    codec: &Codec,
) -> Result<ValidatedForecastCycleCapsule<'a, Codec::Manifest>, ForecastCycleCapsuleError> {
    use ForecastCycleCapsuleError as Error;

    // reject truncated or substituted magic before reading either big-endian field
    if capsule.len() < CAPSULE_HEADER_BYTES || capsule[..8] != CAPSULE_MAGIC {
        return Err(Error::Header);
    }
    let manifest_length = read_u32(&capsule[8..12])? as usize;
    let page_count = read_u32(&capsule[12..16])? as usize;
    let manifest_end = CAPSULE_HEADER_BYTES
        .checked_add(manifest_length)
        .ok_or(Error::TooLarge)?;
    // bound the original persisted manifest before invoking its canonical decoder
    if manifest_length == 0
        || manifest_length > MANIFEST_MAXIMUM_BYTES
        || manifest_end > capsule.len()
    {
        return Err(Error::Manifest);
    }
    let manifest_bytes = &capsule[CAPSULE_HEADER_BYTES..manifest_end];
    let manifest = codec
        .decode_final_manifest(manifest_bytes)
        .ok_or(Error::Manifest)?;
    let layout = codec
        .capsule_manifest_layout(&manifest)
        .ok_or(Error::Manifest)?;
    validate_layout(&layout)?;
    // bind the redundant fixed header count to the validated manifest page sequence
    if page_count != layout.pages.len() {
        return Err(Error::PageCount);
    }

    let mut offset = manifest_end;
    let mut page_payloads = Vec::with_capacity(layout.pages.len());
    // borrow each exact page range and verify its content digest in manifest order
    for page in &layout.pages {
        let end = offset
            .checked_add(page.payload_length)
            .ok_or(Error::TooLarge)?;
        if end > capsule.len() {
            return Err(Error::Truncated);
        }
        let payload = &capsule[offset..end];
        if codec.sha256(payload) != *page.payload_sha256.as_bytes() {
            return Err(Error::PagePayload);
        }
        page_payloads.push(payload);
        offset = end;
    }
    // reject hidden bytes and concatenated second capsules
    if offset != capsule.len() {
        return Err(Error::TrailingBytes);
    }
    let (due_key, generation) = codec.final_manifest_identity(&manifest);
    let due_key = due_key.to_owned();
    Ok(ValidatedForecastCycleCapsule {
        contract_version: FORECAST_CYCLE_CAPSULE_CONTRACT_VERSION,
        due_key,
        generation,
        manifest,
        manifest_bytes,
        manifest_sha256: ForecastCycleSha256::admitted(codec.sha256(manifest_bytes)),
        page_payloads,
        total_payload_bytes: layout.total_payload_bytes,
    })
}

// validate exact contiguous ranges and total payload bounds admitted from the manifest
fn validate_layout(
    layout: &ForecastCycleCapsuleManifestLayout,
) -> Result<(), ForecastCycleCapsuleError> {
    use ForecastCycleCapsuleError as Error;

    // require the same finite nonempty page population as final-manifest validation
    if !(1..=FORECAST_CYCLE_MAXIMUM_PAGES).contains(&layout.pages.len())
        || !(1..=CYCLE_MAXIMUM_PAYLOAD_BYTES).contains(&layout.total_payload_bytes)
    {
        return Err(Error::Manifest);
    }
    let mut expected_offset = 0_u64;
    // reject gaps, overlap, oversized pages, or a total that differs from ordered ranges
    for page in &layout.pages {
        if page.payload_offset != expected_offset
            || !(1..=FORECAST_CYCLE_PAGE_PAYLOAD_MAXIMUM_BYTES).contains(&page.payload_length)
        {
            return Err(Error::Manifest);
        }
        let length: u64 = page
            .payload_length
            .try_into()
            .map_err(|_| Error::Manifest)?;
        expected_offset = expected_offset.checked_add(length).ok_or(Error::TooLarge)?;
    }
    if expected_offset != layout.total_payload_bytes {
        return Err(Error::Manifest);
    }
    Ok(())
}

// decode one fixed big-endian header field without architecture-dependent casts
fn read_u32(bytes: &[u8]) -> Result<u32, ForecastCycleCapsuleError> {
    let bytes: [u8; 4] = bytes
        .try_into()
        .map_err(|_| ForecastCycleCapsuleError::Header)?;
    Ok(u32::from_be_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use crate::revision_archive_canonical::RevisionArchiveCanonicalCodec;

    use super::*;

    const DUE_KEY: &str = "capture/2026-10-08T06:35:00.000Z";

    #[derive(Clone, Debug, Eq, PartialEq)]
    struct Manifest {
        bytes: Vec<u8>,
        layout: ForecastCycleCapsuleManifestLayout,
    }

    struct Codec {
        manifest: Manifest,
    }

    impl RevisionArchiveCanonicalCodec for Codec {
        // this pure capsule fixture does not admit or parse timestamps
        fn canonical_utc_milliseconds(&self, _text: &str) -> Option<i64> {
            None
        }

        // retain a deterministic admitted digest owner without adding a crypto dependency
        fn sha256(&self, bytes: &[u8]) -> [u8; 32] {
            fixture_digest(bytes)
        }
    }

    impl ForecastCycleFinalManifestCodec for Codec {
        type Manifest = Manifest;

        // accept only the fixture's exact canonical manifest bytes
        fn decode_final_manifest(&self, bytes: &[u8]) -> Option<Self::Manifest> {
            (bytes == self.manifest.bytes).then(|| self.manifest.clone())
        }

        // preserve the fixture's already validated canonical bytes
        fn encode_final_manifest(&self, manifest: &Self::Manifest) -> Option<Vec<u8>> {
            Some(manifest.bytes.clone())
        }

        // return the fixed correlation admitted by this bounded fixture
        fn final_manifest_identity<'a>(&self, _manifest: &'a Self::Manifest) -> (&'a str, u64) {
            (DUE_KEY, 1_791_441_300_000)
        }
    }

    impl ForecastCycleCapsuleCodec for Codec {
        // expose only the ranges carried by the already decoded fixture manifest
        fn capsule_manifest_layout(
            &self,
            manifest: &Self::Manifest,
        ) -> Option<ForecastCycleCapsuleManifestLayout> {
            Some(manifest.layout.clone())
        }
    }

    // derive one deterministic non-cryptographic digest for pure boundary tests
    fn fixture_digest(bytes: &[u8]) -> [u8; 32] {
        let mut digest = [0_u8; 32];
        // mix every byte into a stable slot while retaining order sensitivity
        for (index, byte) in bytes.iter().copied().enumerate() {
            let slot = index % digest.len();
            digest[slot] = digest[slot]
                .wrapping_mul(31)
                .wrapping_add(byte)
                .wrapping_add(index as u8);
        }
        digest
    }

    // construct one exact two-page typed manifest projection
    fn fixture() -> (Codec, Vec<u8>, Vec<u8>) {
        let first = b"first-page".to_vec();
        let second = b"second-page".to_vec();
        let layout = ForecastCycleCapsuleManifestLayout {
            pages: vec![
                ForecastCycleCapsulePageRange {
                    payload_offset: 0,
                    payload_length: first.len(),
                    payload_sha256: ForecastCycleSha256::admitted(fixture_digest(&first)),
                },
                ForecastCycleCapsulePageRange {
                    payload_offset: first.len() as u64,
                    payload_length: second.len(),
                    payload_sha256: ForecastCycleSha256::admitted(fixture_digest(&second)),
                },
            ],
            total_payload_bytes: (first.len() + second.len()) as u64,
        };
        (
            Codec {
                manifest: Manifest {
                    bytes: b"{\"fixture\":true}\n".to_vec(),
                    layout,
                },
            },
            first,
            second,
        )
    }

    #[test]
    fn round_trips_exact_header_manifest_and_borrowed_pages() {
        let (codec, first, second) = fixture();
        let manifest_sha256 = ForecastCycleSha256::admitted(codec.sha256(&codec.manifest.bytes));
        let capsule = encode_forecast_cycle_capsule(
            &codec.manifest,
            manifest_sha256,
            &[&first, &second],
            &codec,
        )
        .expect("encode capsule");
        assert_eq!(&capsule[..8], b"WXACYCL2");
        assert_eq!(
            u32::from_be_bytes(capsule[8..12].try_into().expect("manifest length")) as usize,
            codec.manifest.bytes.len()
        );
        assert_eq!(
            u32::from_be_bytes(capsule[12..16].try_into().expect("page count")),
            2
        );

        let validated = validate_forecast_cycle_capsule(&capsule, &codec).expect("validate");
        assert_eq!(
            validated.contract_version,
            FORECAST_CYCLE_CAPSULE_CONTRACT_VERSION
        );
        assert_eq!(validated.due_key, DUE_KEY);
        assert_eq!(validated.manifest_sha256, manifest_sha256);
        assert_eq!(validated.manifest_bytes, codec.manifest.bytes);
        assert_eq!(validated.page_payloads, [&first[..], &second[..]]);
        assert_eq!(
            validated.total_payload_bytes,
            (first.len() + second.len()) as u64
        );
    }

    #[test]
    fn rejects_substitution_truncation_trailing_bytes_and_wrong_manifest_identity() {
        let (codec, first, second) = fixture();
        let manifest_sha256 = ForecastCycleSha256::admitted(codec.sha256(&codec.manifest.bytes));
        let capsule = encode_forecast_cycle_capsule(
            &codec.manifest,
            manifest_sha256,
            &[&first, &second],
            &codec,
        )
        .expect("encode capsule");

        let mut substituted = capsule.clone();
        let last = substituted.last_mut().expect("last page byte");
        *last ^= 1;
        assert!(matches!(
            validate_forecast_cycle_capsule(&substituted, &codec),
            Err(ForecastCycleCapsuleError::PagePayload)
        ));
        assert!(matches!(
            validate_forecast_cycle_capsule(&capsule[..capsule.len() - 1], &codec),
            Err(ForecastCycleCapsuleError::Truncated)
        ));
        let mut trailing = capsule.clone();
        trailing.push(0);
        assert!(matches!(
            validate_forecast_cycle_capsule(&trailing, &codec),
            Err(ForecastCycleCapsuleError::TrailingBytes)
        ));
        assert_eq!(
            encode_forecast_cycle_capsule(
                &codec.manifest,
                ForecastCycleSha256::admitted([0; 32]),
                &[&first, &second],
                &codec,
            ),
            Err(ForecastCycleCapsuleError::ManifestIdentity)
        );
    }
}
