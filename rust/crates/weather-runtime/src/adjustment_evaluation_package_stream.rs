//! Stream adjustment evaluation members without retaining their decoded population.

use std::collections::BTreeSet;
use std::fmt::{self, Display};
use std::io::{self, BufRead, BufReader, Read};

use weather_contracts::JsonValue;
use weather_database::Sha256Digest;

const MAXIMUM_DECODED_ROWS_BYTES: u64 = 48 * 1024 * 1024;
const MAXIMUM_RECORD_BYTES: usize = 4 * 1024 * 1024;
const MAXIMUM_ROWS: u64 = 8192;
const MAXIMUM_RATIO: u64 = 64;
const ROW_READ_BUFFER_BYTES: usize = 16 * 1024;
const MAXIMUM_BODY_COMPRESSED_BYTES: u64 = 2_100_000;
const MAXIMUM_BODY_PLAINTEXT_BYTES: f64 = 2_000_000.0;

/// Incremental state supplied by the approved digest primitive.
pub trait AdjustmentEvaluationMemberDigest {
    /// update the digest with the exact opened byte sequence
    fn update(&mut self, bytes: &[u8]);

    /// finish only the accepted sha256 state
    fn finish(self) -> Option<Sha256Digest>;
}

/// Independent compressed input and incremental digest primitives.
pub trait AdjustmentEvaluationStreamingValues {
    type Digest: AdjustmentEvaluationMemberDigest;
    type Inflated<R: Read>: Read;

    /// create one owned incremental state without retaining the codec owner
    fn begin_sha256(&mut self) -> Self::Digest;

    /// decode every gzip member while pulling from its supplied opened stream
    fn gunzip_stream<R: Read>(&mut self, compressed: R) -> io::Result<Self::Inflated<R>>;
}

/// Retained decoded-text policy applied while a body is pulled.
pub trait AdjustmentEvaluationBodyInspection {
    /// inspect a chunk using the original streaming text boundary and shared tail
    fn contains_private_material(&mut self, bytes: &[u8]) -> bool;

    /// check the text decoder's remaining tail once identity is bound
    fn finish_contains_private_material(self) -> bool;
}

/// One opened regular descriptor bound to the original metadata identity.
pub trait AdjustmentEvaluationOpenedMember: Read {
    /// prove the initial descriptor was regular and listed the declared length
    fn has_declared_size(&self, size_bytes: u64) -> bool;

    /// compare the final descriptor metadata with the initial descriptor
    fn is_unchanged(&self) -> io::Result<bool>;
}

/// Ordinary failures from strict stream consumption.
#[derive(Debug, PartialEq, Eq)]
pub enum AdjustmentEvaluationStreamError {
    Effect(String),
    MemberBytes,
    DecompressionLimit,
    RecordSize,
    EmptyRecord,
    RowCount,
    BodyLimit,
    BodyDecompressionLimit,
    BodyIdentity,
    BodyPrivateMaterial,
}

impl Display for AdjustmentEvaluationStreamError {
    /// preserve the original stream admission diagnostics
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // retain the original phase-specific failure text
        match self {
            Self::Effect(message) => formatter.write_str(message),
            Self::MemberBytes => formatter.write_str("package member bytes differ"),
            Self::DecompressionLimit => {
                formatter.write_str("package row member exceeds the decompression limit")
            }
            Self::RecordSize => formatter.write_str("database export record is empty or oversized"),
            Self::EmptyRecord => formatter.write_str("package row member contains an empty record"),
            Self::RowCount => formatter.write_str("package row count exceeds its limit"),
            Self::BodyLimit => {
                formatter.write_str("rain receipt body exceeds its fixed size limit")
            }
            Self::BodyDecompressionLimit => {
                formatter.write_str("rain receipt body exceeds its decompression limit")
            }
            Self::BodyIdentity => {
                formatter.write_str("rain receipt body hash or decompression ratio is invalid")
            }
            Self::BodyPrivateMaterial => {
                formatter.write_str("rain receipt body contains forbidden private material")
            }
        }
    }
}

impl std::error::Error for AdjustmentEvaluationStreamError {}

impl From<io::Error> for AdjustmentEvaluationStreamError {
    /// preserve a byte limit refusal raised through the decompressor
    fn from(error: io::Error) -> Self {
        // recognize only the member reader's own exact limit signal
        if error
            .get_ref()
            .is_some_and(|source| source.is::<MemberSizeLimit>())
        {
            Self::MemberBytes
        } else {
            Self::Effect(error.to_string())
        }
    }
}

/// Consume an opened member while hashing and counting its exact compressed bytes.
pub fn consume_adjustment_evaluation_member<T>(
    member: impl AdjustmentEvaluationOpenedMember,
    size_bytes: u64,
    sha256: &Sha256Digest,
    digest: impl AdjustmentEvaluationMemberDigest,
    consume: impl FnOnce(&mut dyn Read) -> Result<T, AdjustmentEvaluationStreamError>,
) -> Result<T, AdjustmentEvaluationStreamError> {
    // check the opened descriptor before providing input to the consumer
    if !member.has_declared_size(size_bytes) {
        return Err(AdjustmentEvaluationStreamError::MemberBytes);
    }
    let mut reader = BoundMemberReader {
        member,
        digest,
        size_bytes,
        read_bytes: 0,
    };
    let result = consume(&mut reader)?;
    let BoundMemberReader {
        member,
        digest,
        read_bytes,
        ..
    } = reader;

    // check final identity after the consumer reaches the original stream boundary
    if read_bytes != size_bytes
        || digest.finish().as_ref() != Some(sha256)
        || !member.is_unchanged()?
    {
        return Err(AdjustmentEvaluationStreamError::MemberBytes);
    }
    Ok(result)
}

struct BoundMemberReader<M, D> {
    member: M,
    digest: D,
    size_bytes: u64,
    read_bytes: u64,
}

impl<M: Read, D: AdjustmentEvaluationMemberDigest> Read for BoundMemberReader<M, D> {
    /// hash only bytes admitted by the declared size before exposing them
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        // cap any growth read to one probe byte beyond the declaration
        let limit = usize::try_from(
            self.size_bytes
                .saturating_sub(self.read_bytes)
                .saturating_add(1),
        )
        .unwrap_or(usize::MAX)
        .min(buffer.len());
        let count = self.member.read(&mut buffer[..limit])?;
        let next_bytes = self.read_bytes.saturating_add(count as u64);
        // refuse grown descriptors before forwarding extra bytes
        if next_bytes > self.size_bytes {
            return Err(io::Error::other(MemberSizeLimit));
        }
        self.read_bytes = next_bytes;
        self.digest.update(&buffer[..count]);
        Ok(count)
    }
}

#[derive(Debug)]
struct MemberSizeLimit;

impl Display for MemberSizeLimit {
    /// retain the predecessor diagnostic through a reader error
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        Display::fmt(&AdjustmentEvaluationStreamError::MemberBytes, formatter)
    }
}

impl std::error::Error for MemberSizeLimit {}

/// Bind streamed body plaintext to the original row declaration without retaining it.
pub fn validate_adjustment_evaluation_body(
    compressed: impl Read,
    compressed_bytes: u64,
    expected_bytes: f64,
    expected_sha256: Option<&str>,
    values: &mut impl AdjustmentEvaluationStreamingValues,
    mut inspection: impl AdjustmentEvaluationBodyInspection,
) -> Result<(), AdjustmentEvaluationStreamError> {
    let ratio_bytes = compressed_bytes.saturating_mul(MAXIMUM_RATIO);
    // reject an over-limit row declaration before requesting decoding
    if compressed_bytes > MAXIMUM_BODY_COMPRESSED_BYTES
        || expected_bytes > MAXIMUM_BODY_PLAINTEXT_BYTES
        || expected_bytes > ratio_bytes as f64
    {
        return Err(AdjustmentEvaluationStreamError::BodyLimit);
    }
    let mut inflated = values.gunzip_stream(compressed)?;
    let mut hasher = values.begin_sha256();
    let mut buffer = [0_u8; ROW_READ_BUFFER_BYTES];
    let mut plaintext_bytes = 0_u64;

    // hash and inspect only the current bounded chunk
    loop {
        let count = inflated.read(&mut buffer)?;
        // retain an empty body only when its final binding allows it
        if count == 0 {
            break;
        }
        plaintext_bytes += count as u64;
        // apply both declared plaintext and measured ratio limits
        if plaintext_bytes as f64 > expected_bytes || plaintext_bytes > ratio_bytes {
            return Err(AdjustmentEvaluationStreamError::BodyDecompressionLimit);
        }
        let chunk = &buffer[..count];
        hasher.update(chunk);
        // retain the existing body export policy during the streamed read
        if inspection.contains_private_material(chunk) {
            return Err(AdjustmentEvaluationStreamError::BodyPrivateMaterial);
        }
    }

    // bind the exact count and digest before the decoder's final partial character
    if plaintext_bytes as f64 != expected_bytes
        || !hasher
            .finish()
            .is_some_and(|digest| Some(digest.as_str()) == expected_sha256)
    {
        return Err(AdjustmentEvaluationStreamError::BodyIdentity);
    }
    // apply the last incomplete decoded-text fragment
    if inspection.finish_contains_private_material() {
        return Err(AdjustmentEvaluationStreamError::BodyPrivateMaterial);
    }
    Ok(())
}

/// Minimum row values needed to accumulate an exact original stream summary.
pub struct AdjustmentEvaluationRowObservation<'a> {
    pub revision_count: f64,
    pub source_identity: &'a [u16],
    pub last_received_at: &'a JsonValue,
}

/// A strict row consumer which admits a record before validating its optional body.
pub trait AdjustmentEvaluationRowsConsumer {
    type Row;

    /// validate the complete canonical row line without opening its body
    fn admit_row(&mut self, line: &str) -> Result<Self::Row, AdjustmentEvaluationStreamError>;

    /// project only the admitted row identity fields for stream aggregation
    fn observe_row(row: &Self::Row) -> AdjustmentEvaluationRowObservation<'_>;

    /// compare non-string date values with the retained javascript relation
    fn receipt_is_later(&mut self, next: &JsonValue, prior: &JsonValue) -> bool;

    /// validate a body or retain the admitted row only after count admission
    fn accept_row(&mut self, row: Self::Row) -> Result<(), AdjustmentEvaluationStreamError>;
}

/// Exact decoded stream diagnostics, without retaining the admitted rows themselves.
#[derive(Debug, Default, PartialEq)]
pub struct AdjustmentEvaluationRowStreamSummary {
    pub canonical_stream_bytes: u64,
    pub row_count: u64,
    pub revised_row_count: u64,
    pub max_revision_count: f64,
    pub max_source_receipt_at: Option<JsonValue>,
    pub source_identities: BTreeSet<Vec<u16>>,
    pub ends_with_line_feed: bool,
}

/// Pull one original bounded line at a time from an owned gzip decompressor.
pub fn consume_adjustment_evaluation_row_stream(
    decoded: impl Read,
    compressed_bytes: u64,
    rows: &mut impl AdjustmentEvaluationRowsConsumer,
) -> Result<AdjustmentEvaluationRowStreamSummary, AdjustmentEvaluationStreamError> {
    let maximum_bytes =
        MAXIMUM_DECODED_ROWS_BYTES.min(compressed_bytes.saturating_mul(MAXIMUM_RATIO));
    let mut decoded =
        BufReader::with_capacity(ROW_READ_BUFFER_BYTES, decoded.take(maximum_bytes + 1));
    let mut record = Vec::new();
    let mut summary = AdjustmentEvaluationRowStreamSummary::default();

    // retain and parse only one bounded line instead of the inflate buffer
    loop {
        let chunk = decoded.fill_buf()?;
        // stop only when the decoder has finished without error
        if chunk.is_empty() {
            break;
        }
        let newline = chunk.iter().position(|byte| *byte == b'\n');
        let length = newline.unwrap_or(chunk.len());
        let consumed = length + usize::from(newline.is_some());
        summary.canonical_stream_bytes += consumed as u64;
        // resolve the decoded byte limit before parsing the over-budget record
        if summary.canonical_stream_bytes > maximum_bytes {
            return Err(AdjustmentEvaluationStreamError::DecompressionLimit);
        }
        // bound the raw record before allocating its appended fragment
        if record.len() + length > MAXIMUM_RECORD_BYTES {
            return Err(AdjustmentEvaluationStreamError::RecordSize);
        }
        record.extend_from_slice(&chunk[..length]);
        decoded.consume(consumed);
        summary.ends_with_line_feed = newline.is_some();

        // strip only a cr that directly precedes a recognized newline
        if newline.is_some() {
            let line = record.strip_suffix(b"\r").unwrap_or(&record);
            consume_row(line, rows, &mut summary)?;
            record.clear();
        }
    }

    // admit the final partial record before the caller checks ending framing
    if !record.is_empty() {
        consume_row(&record, rows, &mut summary)?;
    }
    Ok(summary)
}

/// Update summaries before validating or retaining any referenced body.
fn consume_row<Rows: AdjustmentEvaluationRowsConsumer>(
    line: &[u8],
    rows: &mut Rows,
    summary: &mut AdjustmentEvaluationRowStreamSummary,
) -> Result<(), AdjustmentEvaluationStreamError> {
    // keep an empty line distinct from an empty stream
    if line.is_empty() {
        return Err(AdjustmentEvaluationStreamError::EmptyRecord);
    }
    let line = String::from_utf8_lossy(line);
    let row = rows.admit_row(&line)?;
    summary.row_count += 1;
    // keep body effects out of rows past the original count ceiling
    if summary.row_count > MAXIMUM_ROWS {
        return Err(AdjustmentEvaluationStreamError::RowCount);
    }

    let observation = Rows::observe_row(&row);
    summary
        .source_identities
        .insert(observation.source_identity.to_vec());
    summary.revised_row_count += u64::from(observation.revision_count > 0.0);
    summary.max_revision_count = summary.max_revision_count.max(observation.revision_count);
    // compare ordinary utc timestamp strings by their original utf-16 order
    let replace = match (
        observation.last_received_at,
        summary.max_source_receipt_at.as_ref(),
    ) {
        (_, None | Some(JsonValue::Null)) => true,
        (JsonValue::String(next), Some(JsonValue::String(prior))) => next > prior,
        (next, Some(prior)) => rows.receipt_is_later(next, prior),
    };
    // retain only the latest raw receipt value
    if replace {
        summary.max_source_receipt_at = Some(observation.last_received_at.clone());
    }
    rows.accept_row(row)
}

#[cfg(test)]
#[path = "adjustment_evaluation_package_stream_test.rs"]
mod tests;
