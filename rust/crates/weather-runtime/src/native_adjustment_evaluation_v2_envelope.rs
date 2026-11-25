//! Native streaming owner for the adjustment evaluation v2 envelope around an unchanged v1 archive.

use std::{
    fs::{File, Metadata},
    io::{self, Write},
    os::unix::fs::{FileExt, MetadataExt},
    path::Path,
};

use jiff::{Span, civil::Date};
use sha2::{Digest, Sha256};
use weather_contracts::JsonValue;
use weather_edge::ArchiveWirePrimitives;

use crate::{
    NativeWirePrimitives, admit_native_adjustment_confirmation_authorization_v2,
    api_maintenance_candidate_files::open_read_no_follow,
    maintenance_canonical_json::{exact_keys, field, text, valid_hash},
    read_native_adjustment_confirmation_authorization_dates,
};

const MAXIMUM_ARCHIVE_BYTES: u64 = 48 * 1024 * 1024;
const MAXIMUM_HEADER_BYTES: usize = 64 * 1024;
const MAXIMUM_AUTHORIZATION_BYTES: u64 = 768 * 1024;
const MAXIMUM_EDGE_SNAPSHOT_BYTES: u64 = 4 * 1024 * 1024;
const COPY_BUFFER_BYTES: usize = 256 * 1024;
const STREAM_MAGIC: &[u8] = b"weather-adjustment-evaluation-envelope/v2\n";
const ENVELOPE_VERSION: &str = "adjustment-evaluation-export-envelope/v2";
const SOURCE_PACKAGE_VERSION: &str = "adjustment-evaluation-export-package/v1";
const EDGE_SNAPSHOT_VERSION: &str = "forecast-adjustment-evidence-snapshot/v1";
const HEADER_FIELDS: &[&str] = &[
    "authorization",
    "authorizationSha256",
    "contractVersion",
    "dateSemantics",
    "exportKind",
    "fromLocalDate",
    "payloadBytes",
    "payloadSha256",
    "snapshotLineage",
    "sourcePackageContractVersion",
    "toLocalDate",
    "valueAuthority",
];
const EDGE_SNAPSHOT_FIELDS: &[&str] =
    &["contractVersion", "entries", "frozenAt", "watermarkSha256"];

/// Closed source distinction between descriptive daily transport and persisted post-burn authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeAdjustmentEvaluationV2ExportKind {
    DailyMonitoring,
    PostBurnConfirmation,
}

impl NativeAdjustmentEvaluationV2ExportKind {
    /// Return the exact source wire spelling.
    pub const fn wire(self) -> &'static str {
        match self {
            Self::DailyMonitoring => "daily_monitoring",
            Self::PostBurnConfirmation => "post_burn_confirmation",
        }
    }
}

/// Fixed-path inputs for one bounded v2 stream around a previously produced v1 archive.
#[derive(Clone, Copy, Debug)]
pub struct NativeAdjustmentEvaluationV2EnvelopeInput<'a> {
    pub authorization_path: Option<&'a Path>,
    pub edge_snapshot_path: &'a Path,
    pub export_kind: NativeAdjustmentEvaluationV2ExportKind,
    pub from_local_date: &'a str,
    pub source_archive_path: &'a Path,
    pub to_local_date: &'a str,
}

/// Canonical header and immutable v1 payload identity emitted by one completed stream.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeAdjustmentEvaluationV2EnvelopeOutcome {
    pub header: JsonValue,
    pub header_bytes: Vec<u8>,
    pub payload_bytes: u64,
    pub payload_sha256: String,
}

struct SourceArchive {
    identity: FileIdentity,
    sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct FileIdentity {
    ctime: i64,
    ctime_nsec: i64,
    dev: u64,
    ino: u64,
    len: u64,
    mtime: i64,
    mtime_nsec: i64,
}

struct Authorization {
    envelope: JsonValue,
    envelope_sha256: String,
    from_local_date: String,
    revision_catalog_watermark_sha256: String,
    target_comparator_snapshot_root_sha256: String,
    to_local_date: String,
}

/// Prehash, frame, and stream one stable v1 archive under the exact v2 envelope contract.
pub fn stream_native_adjustment_evaluation_v2_envelope(
    input: NativeAdjustmentEvaluationV2EnvelopeInput<'_>,
    output: &mut impl Write,
) -> io::Result<NativeAdjustmentEvaluationV2EnvelopeOutcome> {
    validate_date_range(input.from_local_date, input.to_local_date)?;
    let source = hash_source_archive(input.source_archive_path)?;
    let edge_watermark = read_edge_watermark(input.edge_snapshot_path)?;
    let authorization = match (input.export_kind, input.authorization_path) {
        (NativeAdjustmentEvaluationV2ExportKind::DailyMonitoring, None) => None,
        (NativeAdjustmentEvaluationV2ExportKind::PostBurnConfirmation, Some(path)) => {
            Some(read_authorization(path)?)
        }
        _ => return Err(invalid_envelope()),
    };
    // bind confirmation dates to the independently persisted burn authorization
    if authorization.as_ref().is_some_and(|authorization| {
        authorization.from_local_date != input.from_local_date
            || authorization.to_local_date != input.to_local_date
    }) {
        return Err(invalid_envelope());
    }
    let header = build_header(input, &source, &edge_watermark, authorization.as_ref());
    // defend the source field set before canonical framing
    if !exact_keys(&header, HEADER_FIELDS) {
        return Err(invalid_envelope());
    }
    let header_bytes = NativeWirePrimitives
        .encode_sorted_json(&header)
        .ok_or_else(invalid_envelope)?;
    // reject a future expanded authorization before emitting any stream bytes
    if header_bytes.len() > MAXIMUM_HEADER_BYTES {
        return Err(invalid_envelope());
    }
    output.write_all(STREAM_MAGIC)?;
    output.write_all(format!("{:08x}\n", header_bytes.len()).as_bytes())?;
    output.write_all(&header_bytes)?;
    stream_source_archive(input.source_archive_path, &source, output)?;
    Ok(NativeAdjustmentEvaluationV2EnvelopeOutcome {
        header,
        header_bytes,
        payload_bytes: source.identity.len,
        payload_sha256: source.sha256,
    })
}

/// Prehash one bounded no-follow archive and retain its exact descriptor identity.
fn hash_source_archive(path: &Path) -> io::Result<SourceArchive> {
    let file = open_read_no_follow(path)?;
    let before = file.metadata()?;
    let identity = file_identity(&before);
    // require one nonempty regular v1 archive within the original cap
    if !before.is_file() || identity.len == 0 || identity.len > MAXIMUM_ARCHIVE_BYTES {
        return Err(invalid_envelope());
    }
    let digest = hash_file_range(&file, identity.len)?;
    let after = file_identity(&file.metadata()?);
    // refuse replacement or mutation during the prehash pass
    if identity != after {
        return Err(invalid_envelope());
    }
    Ok(SourceArchive {
        identity,
        sha256: lower_hex(digest),
    })
}

/// Stream only the prehashed descriptor identity and recheck it after the copy.
fn stream_source_archive(
    path: &Path,
    source: &SourceArchive,
    output: &mut impl Write,
) -> io::Result<()> {
    let file = open_read_no_follow(path)?;
    let before = file_identity(&file.metadata()?);
    // prohibit path replacement between prehash and stream passes
    if before != source.identity {
        return Err(invalid_envelope());
    }
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; COPY_BUFFER_BYTES];
    let mut offset = 0u64;
    // copy the exact proven source range without buffering the archive
    while offset < source.identity.len {
        let remaining =
            usize::try_from((source.identity.len - offset).min(COPY_BUFFER_BYTES as u64))
                .map_err(|_| invalid_envelope())?;
        let count = file.read_at(&mut buffer[..remaining], offset)?;
        // reject an early end instead of emitting a short frame
        if count == 0 {
            return Err(invalid_envelope());
        }
        hasher.update(&buffer[..count]);
        output.write_all(&buffer[..count])?;
        offset = offset
            .checked_add(u64::try_from(count).map_err(|_| invalid_envelope())?)
            .ok_or_else(invalid_envelope)?;
    }
    let after = file_identity(&file.metadata()?);
    // close the second race window and the streamed payload identity together
    if before != after || lower_hex(hasher.finalize().into()) != source.sha256 {
        return Err(invalid_envelope());
    }
    Ok(())
}

/// Hash one fixed descriptor range with the source's 256 KiB streaming bound.
fn hash_file_range(file: &File, length: u64) -> io::Result<[u8; 32]> {
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; COPY_BUFFER_BYTES];
    let mut offset = 0u64;
    // consume the exact stat-proven range
    while offset < length {
        let remaining = usize::try_from((length - offset).min(COPY_BUFFER_BYTES as u64))
            .map_err(|_| invalid_envelope())?;
        let count = file.read_at(&mut buffer[..remaining], offset)?;
        // reject an unexpected early end
        if count == 0 {
            return Err(invalid_envelope());
        }
        hasher.update(&buffer[..count]);
        offset = offset
            .checked_add(u64::try_from(count).map_err(|_| invalid_envelope())?)
            .ok_or_else(invalid_envelope)?;
    }
    Ok(hasher.finalize().into())
}

/// Read one stable bounded control file without following its final component.
fn read_stable_file(path: &Path, maximum: u64) -> io::Result<Vec<u8>> {
    let file = open_read_no_follow(path)?;
    let metadata = file.metadata()?;
    let before = file_identity(&metadata);
    // bound allocation to a regular descriptor observed after opening
    if !metadata.is_file() || before.len > maximum {
        return Err(invalid_envelope());
    }
    let length = usize::try_from(before.len).map_err(|_| invalid_envelope())?;
    let mut bytes = vec![0; length];
    let mut offset = 0usize;
    // fill the descriptor range proven by the first metadata observation
    while offset < length {
        let count = file.read_at(
            &mut bytes[offset..],
            u64::try_from(offset).map_err(|_| invalid_envelope())?,
        )?;
        // reject truncated control material
        if count == 0 {
            return Err(invalid_envelope());
        }
        offset = offset.checked_add(count).ok_or_else(invalid_envelope)?;
    }
    let mut probe = [0u8; 1];
    let extra = file.read_at(&mut probe, before.len)?;
    let after = file_identity(&file.metadata()?);
    // reject growth, replacement, or metadata mutation during the read
    if extra != 0 || before != after {
        return Err(invalid_envelope());
    }
    Ok(bytes)
}

/// Admit the frozen edge snapshot contract and return only its watermark identity.
fn read_edge_watermark(path: &Path) -> io::Result<String> {
    let bytes = read_stable_file(path, MAXIMUM_EDGE_SNAPSHOT_BYTES)?;
    let value = NativeWirePrimitives
        .decode_json(&bytes)
        .ok_or_else(invalid_envelope)?;
    // preserve the exact closed snapshot fields and frozen edge identity
    if !exact_keys(&value, EDGE_SNAPSHOT_FIELDS)
        || text(&value, "contractVersion").as_deref() != Some(EDGE_SNAPSHOT_VERSION)
        || !matches!(field(&value, "entries"), Some(JsonValue::Array(_)))
        || text(&value, "frozenAt")
            .as_deref()
            .and_then(|value| NativeWirePrimitives.parse_explicit_utc_milliseconds(value))
            .is_none()
    {
        return Err(invalid_envelope());
    }
    let watermark = text(&value, "watermarkSha256").ok_or_else(invalid_envelope)?;
    valid_hash(&watermark)
        .then_some(watermark)
        .ok_or_else(invalid_envelope)
}

/// Read and fully validate one canonical persisted-burn authorization envelope.
fn read_authorization(path: &Path) -> io::Result<Authorization> {
    let bytes = read_stable_file(path, MAXIMUM_AUTHORIZATION_BYTES)?;
    let envelope = admit_native_adjustment_confirmation_authorization_v2(&bytes)
        .map_err(|_| invalid_envelope())?;
    let (from_local_date, to_local_date) =
        read_native_adjustment_confirmation_authorization_dates(&bytes)
            .map_err(|_| invalid_envelope())?;
    let payload = field(&envelope, "payload").ok_or_else(invalid_envelope)?;
    let revision_catalog_watermark_sha256 =
        text(payload, "revisionCatalogWatermarkSha256").ok_or_else(invalid_envelope)?;
    let target_comparator_snapshot_root_sha256 =
        text(payload, "targetComparatorSnapshotRootSha256").ok_or_else(invalid_envelope)?;
    Ok(Authorization {
        envelope,
        envelope_sha256: lower_hex(Sha256::digest(&bytes).into()),
        from_local_date,
        revision_catalog_watermark_sha256,
        target_comparator_snapshot_root_sha256,
        to_local_date,
    })
}

/// Construct the exact canonical stream header for either closed authority state.
fn build_header(
    input: NativeAdjustmentEvaluationV2EnvelopeInput<'_>,
    source: &SourceArchive,
    edge_watermark: &str,
    authorization: Option<&Authorization>,
) -> JsonValue {
    let (authorization_value, authorization_sha256, value_authority, revision, target) =
        match authorization {
            Some(authorization) => (
                authorization.envelope.clone(),
                string(&authorization.envelope_sha256),
                "persisted_burn_authorization",
                string(&authorization.revision_catalog_watermark_sha256),
                string(&authorization.target_comparator_snapshot_root_sha256),
            ),
            None => (
                JsonValue::Null,
                JsonValue::Null,
                "none",
                JsonValue::Null,
                JsonValue::Null,
            ),
        };
    JsonValue::Object(vec![
        member("authorization", authorization_value),
        member("authorizationSha256", authorization_sha256),
        member("contractVersion", string(ENVELOPE_VERSION)),
        member(
            "dateSemantics",
            string("america_los_angeles_inclusive_local_dates"),
        ),
        member("exportKind", string(input.export_kind.wire())),
        member("fromLocalDate", string(input.from_local_date)),
        member(
            "payloadBytes",
            JsonValue::Number(source.identity.len as f64),
        ),
        member("payloadSha256", string(&source.sha256)),
        member(
            "snapshotLineage",
            JsonValue::Object(vec![
                member("edgeEvidenceWatermarkSha256", string(edge_watermark)),
                member("revisionCatalogWatermarkSha256", revision),
                member("targetComparatorSnapshotRootSha256", target),
            ]),
        ),
        member(
            "sourcePackageContractVersion",
            string(SOURCE_PACKAGE_VERSION),
        ),
        member("toLocalDate", string(input.to_local_date)),
        member("valueAuthority", string(value_authority)),
    ])
}

/// Validate one inclusive one-to-fourteen-day source export range.
fn validate_date_range(from: &str, to: &str) -> io::Result<()> {
    let from = calendar_date(from)?;
    let to = calendar_date(to)?;
    // find the exact inclusive span without assuming timezone day length
    for days in 0..14 {
        let candidate = from
            .checked_add(Span::new().try_days(days).map_err(|_| invalid_envelope())?)
            .map_err(|_| invalid_envelope())?;
        // return only the first exact admitted inclusive span
        if candidate == to {
            return Ok(());
        }
    }
    Err(invalid_envelope())
}

/// Parse only the source's canonical four-digit Gregorian date spelling.
fn calendar_date(value: &str) -> io::Result<Date> {
    let date = value.parse::<Date>().map_err(|_| invalid_envelope())?;
    // reject normalized or extended-year aliases
    if value.len() != 10 || date.to_string() != value {
        return Err(invalid_envelope());
    }
    Ok(date)
}

/// Snapshot the source metadata fields checked by both original passes.
fn file_identity(metadata: &Metadata) -> FileIdentity {
    FileIdentity {
        ctime: metadata.ctime(),
        ctime_nsec: metadata.ctime_nsec(),
        dev: metadata.dev(),
        ino: metadata.ino(),
        len: metadata.len(),
        mtime: metadata.mtime(),
        mtime_nsec: metadata.mtime_nsec(),
    }
}

/// Encode one digest with the source's lowercase spelling.
fn lower_hex(bytes: [u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(64);
    // encode both nibbles of every digest byte in order
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

/// Construct one generated object member.
fn member(key: &str, value: JsonValue) -> (Vec<u16>, JsonValue) {
    (key.encode_utf16().collect(), value)
}

/// Construct one generated string.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Collapse malformed control, archive, or stream material to one fixed refusal.
fn invalid_envelope() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "adjustment evaluation v2 envelope is invalid",
    )
}
