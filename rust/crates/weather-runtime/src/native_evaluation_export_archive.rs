//! Bounded native producer for the original adjustment evaluation snapshot and v1 archive.

use std::collections::BTreeSet;
use std::io::{self, BufRead, Read, Write};
use std::path::{Path, PathBuf};

use weather_contracts::JsonValue;
use weather_database::Sha256Digest;
use weather_edge::{
    ArchiveWirePrimitives, ForecastEvidenceMaterial, ForecastEvidenceObjectVersion,
    NativeForecastSnapshotCatalog, RevisionArchiveCanonicalClock, RevisionArchiveCanonicalCodec,
};

use crate::adjustment_evaluation_package_manifest::validate_database_manifest;
use crate::maintenance_canonical_json::{exact_keys, field, text, valid_hash};
use crate::{
    AdjustmentEvaluationPackageManifestValues, AdjustmentEvaluationPackageRowValues,
    MaintenanceCanonicalJson, NativeAdjustmentEvaluationPackageFiles,
    NativeEvaluationStreamingCodec, NativeForecastArchiveCodec, NativeNodeGzipWriter,
    NativeWirePrimitives, admit_adjustment_evaluation_package_row, native_node_gzip_candidate,
};

const MIB: usize = 1024 * 1024;
const MAX_ROWS: usize = 8_192;
const MAX_RECORD: usize = 4 * MIB;
const MAX_CANONICAL: usize = 48 * MIB;
const MAX_BODY: usize = 32 * MIB;
const MAX_ARCHIVE: usize = 48 * MIB;
const MAX_MEMBERS: usize = 64 * MIB;
const MAX_OTHER: usize = 16 * MIB;
const MAX_TAR: usize = MAX_MEMBERS + (3 * MAX_ROWS + 4) * 1024;

/// Freeze the exact native forecast receipt/object catalog before opening the DB transaction.
pub fn freeze_native_evaluation_edge_snapshot(
    root: &Path,
    clock: &mut impl RevisionArchiveCanonicalClock,
) -> io::Result<Vec<u8>> {
    let material = material();
    let snapshot = NativeForecastSnapshotCatalog::new(root.to_path_buf())
        .and_then(|reader| reader.freeze(&material, clock))
        .map_err(|_| invalid("adjustment evidence snapshot differs"))?;
    let mut entries = Vec::with_capacity(snapshot.entries().len());
    // retain the original host-only paths and portable ordered digest pairs
    for entry in snapshot.entries() {
        entries.push(object(vec![
            (
                "edgeReceiptIdentitySha256",
                string(&hex(entry.edge_receipt_identity_sha256.as_bytes())),
            ),
            (
                "objectPath",
                string(
                    entry
                        .object_path
                        .to_str()
                        .ok_or_else(|| invalid("edge object path differs"))?,
                ),
            ),
            ("objectSha256", string(&hex(entry.object_sha256.as_bytes()))),
            (
                "receiptPath",
                string(
                    entry
                        .receipt_path
                        .to_str()
                        .ok_or_else(|| invalid("edge receipt path differs"))?,
                ),
            ),
        ]));
    }
    canonical(&object(vec![
        ("contractVersion", string(snapshot.contract_version())),
        ("entries", JsonValue::Array(entries)),
        ("frozenAt", string(snapshot.frozen_at())),
        (
            "watermarkSha256",
            string(&hex(snapshot.watermark_sha256().as_bytes())),
        ),
    ]))
}

/// Produce the exact fixed-order tar archive from bounded DB records and a frozen edge snapshot.
pub fn stream_native_evaluation_archive_v1(
    mut input: impl BufRead,
    snapshot_bytes: &[u8],
    edge_root: &Path,
    from: &str,
    to: &str,
    output: impl Write,
) -> io::Result<String> {
    let mut values = ExportValues::new()?;
    let dates = date_ordinal(to).zip(date_ordinal(from));
    // require the original inclusive one-to-fourteen local dates
    if !dates.is_some_and(|(end, start)| (1..=14).contains(&(end - start + 1))) {
        return Err(invalid("adjustment export dates differ"));
    }
    let mut rows = NativeNodeGzipWriter::new(BoundedSink::new(Vec::new(), MAX_OTHER))?;
    let mut canonical_bytes = 0usize;
    let mut archive = TarArchive::new(output)?;
    let mut database = None;
    let mut transaction = None;
    let mut bodies = BTreeSet::new();
    let mut sources = BTreeSet::new();
    let mut row_count = 0usize;
    let mut revised = 0usize;
    let mut max_revision = 0f64;
    let mut body_bytes = 0usize;
    let mut maximum_receipt: Option<Vec<u16>> = None;
    // consume one exact leading manifest then each database row within its own byte limit
    loop {
        let mut line = Vec::new();
        let count = (&mut input)
            .take((MAX_RECORD + 1) as u64)
            .read_until(b'\n', &mut line)?;
        if count == 0 {
            break;
        }
        if line.len() > MAX_RECORD || line.last() != Some(&b'\n') {
            return Err(invalid("database export record exceeds its bound"));
        }
        let record = NativeWirePrimitives
            .decode_json(&line)
            .ok_or_else(|| invalid("database export record differs"))?;
        let kind = text(&record, "record_type")
            .ok_or_else(|| invalid("database export record kind differs"))?;
        if kind == "manifest" {
            if database.is_some()
                || row_count != 0
                || !exact_keys(&record, &["payload", "record_type", "transaction"])
            {
                return Err(invalid("database export manifest ordering differs"));
            }
            let manifest = field(&record, "payload")
                .ok_or_else(|| invalid("database manifest unavailable"))?;
            validate_database_manifest(manifest, &mut values).map_err(io::Error::other)?;
            let settings = field(&record, "transaction")
                .ok_or_else(|| invalid("database transaction unavailable"))?;
            transaction = Some(project_transaction(settings, from, to)?);
            database = Some(manifest.clone());
            continue;
        }
        if kind != "row"
            || database.is_none()
            || !exact_keys(&record, &["payload", "record_type"])
            || row_count >= MAX_ROWS
        {
            return Err(invalid("database export row ordering differs"));
        }
        let mut row = field(&record, "payload")
            .ok_or_else(|| invalid("database row unavailable"))?
            .clone();
        let body_value = field(&row, "compressed_body_base64")
            .ok_or_else(|| invalid("database row body field differs"))?;
        let body = match body_value {
            JsonValue::Null => None,
            JsonValue::String(units) => {
                if text(&row, "record_kind").as_deref() != Some("rain_receipt") {
                    return Err(invalid("database body record kind differs"));
                }
                let encoded = String::from_utf16(units)
                    .map_err(io::Error::other)?
                    .replace('\n', "");
                let bytes = NativeWirePrimitives
                    .decode_standard_base64(&encoded)
                    .ok_or_else(|| invalid("database body encoding differs"))?;
                if NativeWirePrimitives
                    .encode_standard_base64(&bytes)
                    .as_deref()
                    != Some(encoded.as_str())
                {
                    return Err(invalid("database body encoding differs"));
                }
                let payload = field(&row, "payload")
                    .ok_or_else(|| invalid("database body metadata unavailable"))?;
                let digest = text(payload, "bodySha256")
                    .filter(|hash| valid_hash(hash))
                    .ok_or_else(|| invalid("database body digest differs"))?;
                let expected_plain = integer(payload, "bodyBytes")
                    .ok_or_else(|| invalid("database body length differs"))?;
                if text(&row, "content_hash").as_deref() != Some(digest.as_str())
                    || integer(payload, "compressedBytes") != Some(bytes.len())
                    || bytes.len() > 2_100_000
                    || expected_plain > 2_000_000
                    || expected_plain > bytes.len().saturating_mul(64)
                {
                    return Err(invalid("database body length differs"));
                }
                let plain = NativeEvaluationStreamingCodec
                    .gunzip_bounded(&bytes, expected_plain)
                    .ok_or_else(|| invalid("database body compression differs"))?;
                if plain.len() != expected_plain
                    || sha(&plain) != digest
                    || values.private.is_match(&String::from_utf8_lossy(&plain))
                {
                    return Err(invalid("database body identity differs"));
                }
                Some((digest, bytes))
            }
            _ => return Err(invalid("database body encoding differs")),
        };
        let body_path = body
            .as_ref()
            .map(|(digest, _)| format!("members/bodies/sha256-{digest}.json.gz"));
        let JsonValue::Object(ref mut fields) = row else {
            return Err(invalid("database row shape differs"));
        };
        fields.retain(|(key, _)| {
            !key.iter()
                .copied()
                .eq("compressed_body_base64".encode_utf16())
        });
        fields.push(member(
            "body_member_path",
            body_path.as_deref().map_or(JsonValue::Null, string),
        ));
        let bytes = canonical(&row)?;
        let admitted = admit_adjustment_evaluation_package_row(
            bytes
                .strip_suffix(b"\n")
                .ok_or_else(|| invalid("row framing differs"))?,
            &mut values,
        )
        .map_err(io::Error::other)?;
        if let Some((digest, bytes)) = body
            && bodies.insert(digest)
        {
            body_bytes = body_bytes
                .checked_add(bytes.len())
                .ok_or_else(|| invalid("body limit"))?;
            if body_bytes > MAX_BODY {
                return Err(invalid("compressed body limit"));
            }
            archive.append(
                body_path
                    .as_deref()
                    .ok_or_else(|| invalid("body path differs"))?,
                &bytes,
            )?;
        }
        // stream each logical row without buffering the whole canonical input
        canonical_bytes = canonical_bytes
            .checked_add(bytes.len())
            .ok_or_else(|| invalid("canonical row limit"))?;
        if canonical_bytes > MAX_CANONICAL {
            return Err(invalid("canonical row limit"));
        }
        rows.write_all(&bytes)?;
        row_count += 1;
        if admitted.revision_count > 0.0 {
            revised += 1;
        }
        max_revision = max_revision.max(admitted.revision_count);
        sources.insert(admitted.source_identity);
        if let JsonValue::String(receipt) = admitted.last_received_at
            && maximum_receipt
                .as_ref()
                .is_none_or(|current| current < &receipt)
        {
            maximum_receipt = Some(receipt);
        }
    }
    let database = database.ok_or_else(|| invalid("database export manifest unavailable"))?;
    let transaction =
        transaction.ok_or_else(|| invalid("database export transaction unavailable"))?;
    // retain only the bounded compressed row member until it is emitted
    let compressed_rows = rows.finish()?.inner;
    let edge = append_edge(&mut archive, snapshot_bytes, edge_root)?;
    archive.append("members/rows.jsonl.gz", &compressed_rows)?;
    drop(compressed_rows);
    archive.members.sort_by_key(|member| text(member, "path"));
    let manifest = object(vec![
        ("canonicalStreamBytes", number(canonical_bytes)),
        ("compressedBodyBytes", number(body_bytes)),
        (
            "contractVersion",
            string("adjustment-evaluation-export-package/v1"),
        ),
        ("databaseManifest", database),
        ("edgeEvidence", edge),
        ("fromLocalDate", string(from)),
        (
            "limits",
            object(vec![
                ("maxArchiveBytes", number(MAX_ARCHIVE)),
                ("maxCanonicalStreamBytes", number(MAX_CANONICAL)),
                ("maxCompressedBodyBytes", number(MAX_BODY)),
                ("maxDays", number(14)),
                ("maxDecompressionRatio", number(64)),
                ("maxOtherSimultaneousTemporaryBytes", number(MAX_OTHER)),
                ("maxRows", number(MAX_ROWS)),
                ("maxSimultaneousHostTemporaryBytes", number(MAX_MEMBERS)),
            ]),
        ),
        (
            "maxSourceReceiptAt",
            maximum_receipt.map_or(JsonValue::Null, JsonValue::String),
        ),
        (
            "members",
            JsonValue::Array(std::mem::take(&mut archive.members)),
        ),
        (
            "revisionDiagnostics",
            object(vec![
                ("maxRevisionCount", JsonValue::Number(max_revision)),
                ("revisedRowCount", number(revised)),
                ("rowCount", number(row_count)),
            ]),
        ),
        ("siteKey", string("ballydidean")),
        ("siteTimezone", string("America/Los_Angeles")),
        (
            "sourceIdentities",
            JsonValue::Array(sources.into_iter().map(JsonValue::String).collect()),
        ),
        ("toLocalDate", string(to)),
        ("transaction", transaction),
    ]);
    let manifest_bytes = canonical(&manifest)?;
    let digest = sha(&manifest_bytes);
    archive.append("manifest.json", &manifest_bytes)?;
    archive.append("manifest.sha256", format!("{digest}\n").as_bytes())?;
    archive.finish()?;
    Ok(digest)
}

/// Rebind each frozen edge pair to the original immutable file names and deterministic bytes.
fn append_edge(
    archive: &mut TarArchive<impl Write>,
    bytes: &[u8],
    root: &Path,
) -> io::Result<JsonValue> {
    let snapshot = NativeWirePrimitives
        .decode_json(bytes)
        .ok_or_else(|| invalid("edge snapshot differs"))?;
    if !exact_keys(
        &snapshot,
        &["contractVersion", "entries", "frozenAt", "watermarkSha256"],
    ) || text(&snapshot, "contractVersion").as_deref()
        != Some("forecast-adjustment-evidence-snapshot/v1")
    {
        return Err(invalid("edge snapshot identity differs"));
    }
    let frozen = text(&snapshot, "frozenAt")
        .filter(|value| {
            NativeWirePrimitives
                .parse_explicit_utc_milliseconds(value)
                .is_some()
        })
        .ok_or_else(|| invalid("edge snapshot clock differs"))?;
    let watermark = text(&snapshot, "watermarkSha256")
        .filter(|hash| valid_hash(hash))
        .ok_or_else(|| invalid("edge snapshot watermark differs"))?;
    let Some(JsonValue::Array(entries)) = field(&snapshot, "entries") else {
        return Err(invalid("edge snapshot entries differ"));
    };
    if entries.len() > MAX_ROWS {
        return Err(invalid("edge snapshot count differs"));
    }
    let mut seen_receipts = BTreeSet::new();
    let mut seen_objects = BTreeSet::new();
    let mut watermark_pairs = Vec::with_capacity(entries.len());
    let material = material();
    let root_text = root.to_str().ok_or_else(|| invalid("edge root differs"))?;
    for entry in entries {
        if !exact_keys(
            entry,
            &[
                "edgeReceiptIdentitySha256",
                "objectPath",
                "objectSha256",
                "receiptPath",
            ],
        ) {
            return Err(invalid("edge entry differs"));
        }
        let receipt = text(entry, "edgeReceiptIdentitySha256")
            .filter(|hash| valid_hash(hash))
            .ok_or_else(|| invalid("edge receipt differs"))?;
        let object_sha = text(entry, "objectSha256")
            .filter(|hash| valid_hash(hash))
            .ok_or_else(|| invalid("edge object differs"))?;
        // rebind the frozen portable pair sequence without copying object payloads
        if !seen_receipts.insert(receipt.clone()) {
            return Err(invalid("edge receipt duplicate"));
        }
        watermark_pairs.push(object(vec![
            ("edgeReceiptIdentitySha256", string(&receipt)),
            ("objectSha256", string(&object_sha)),
        ]));
        let receipt_relative = format!("receipts/sha256-{receipt}.json");
        let object_relative = format!("objects/sha256-{object_sha}.json.gz");
        if text(entry, "receiptPath").map(PathBuf::from) != Some(root.join(&receipt_relative))
            || text(entry, "objectPath").map(PathBuf::from) != Some(root.join(&object_relative))
        {
            return Err(invalid("edge member path differs"));
        }
        let receipt_bytes = NativeAdjustmentEvaluationPackageFiles.read_bounded_control(
            root_text,
            &receipt_relative,
            MIB as u64,
            "edge receipt",
        )?;
        let object_bytes = NativeAdjustmentEvaluationPackageFiles.read_bounded_control(
            root_text,
            &object_relative,
            MIB as u64,
            "edge object",
        )?;
        let receipt_id = weather_edge::RevisionGapSha256::from_lower_hex(&receipt)
            .ok_or_else(|| invalid("edge receipt digest differs"))?;
        let object_id = weather_edge::RevisionGapSha256::from_lower_hex(&object_sha)
            .ok_or_else(|| invalid("edge object digest differs"))?;
        let receipt_claim = material
            .admit_retained_receipt(*receipt_id.as_bytes(), &receipt_bytes)
            .ok_or_else(|| invalid("edge receipt differs"))?;
        let object_claim = material
            .admit_retained_object(*object_id.as_bytes(), &object_bytes)
            .ok_or_else(|| invalid("edge object differs"))?;
        if receipt_claim.object_sha256 != *object_id.as_bytes()
            || receipt_claim.availability_rows != object_claim.row_count
            || receipt_claim.window != object_claim.window
            || receipt_claim.version != ForecastEvidenceObjectVersion::Legacy
            || object_claim.version != receipt_claim.version
            || object_claim.stable_receipt_identity_sha256 != Some(*receipt_id.as_bytes())
        {
            return Err(invalid("edge receipt pairing differs"));
        }
        archive.append(&format!("members/edge/{receipt_relative}"), &receipt_bytes)?;
        if seen_objects.insert(object_sha) {
            archive.append(&format!("members/edge/{object_relative}"), &object_bytes)?;
        }
    }
    // confirm the exact frozen snapshot watermark from its ordered portable addresses
    let watermark_bytes = canonical(&JsonValue::Array(watermark_pairs))?;
    // the evidence-store watermark hashes its canonical value without the package LF
    if sha(watermark_bytes
        .strip_suffix(b"\n")
        .ok_or_else(|| invalid("edge watermark framing differs"))?)
        != watermark
    {
        return Err(invalid("edge snapshot watermark differs"));
    }
    Ok(object(vec![
        (
            "contractVersion",
            string("forecast-adjustment-evidence-snapshot/v1"),
        ),
        ("frozenAt", string(&frozen)),
        ("objectCount", number(seen_objects.len())),
        ("receiptCount", number(seen_receipts.len())),
        ("watermarkSha256", string(&watermark)),
    ]))
}

/// Validate the exact read-only SQL envelope and project original manifest transaction keys.
fn project_transaction(value: &JsonValue, from: &str, to: &str) -> io::Result<JsonValue> {
    let names = [
        ("createdAtUtc", "created_at_utc"),
        (
            "idleInTransactionSessionTimeout",
            "idle_in_transaction_session_timeout",
        ),
        ("isolationLevel", "isolation_level"),
        ("lockTimeout", "lock_timeout"),
        ("readOnly", "read_only"),
        ("statementTimeout", "statement_timeout"),
    ];
    if !exact_keys(
        value,
        &[
            "created_at_utc",
            "from_local_date",
            "idle_in_transaction_session_timeout",
            "isolation_level",
            "lock_timeout",
            "read_only",
            "statement_timeout",
            "to_local_date",
        ],
    ) || text(value, "from_local_date").as_deref() != Some(from)
        || text(value, "to_local_date").as_deref() != Some(to)
        || [
            ("idle_in_transaction_session_timeout", "30s"),
            ("isolation_level", "repeatable read"),
            ("lock_timeout", "5s"),
            ("read_only", "on"),
            ("statement_timeout", "5min"),
        ]
        .iter()
        .any(|(k, v)| text(value, k).as_deref() != Some(*v))
        || text(value, "created_at_utc")
            .and_then(|v| NativeWirePrimitives.parse_explicit_utc_milliseconds(&v))
            .is_none()
    {
        return Err(invalid("database transaction differs"));
    }
    Ok(object(
        names
            .into_iter()
            .map(|(name, source)| (name, field(value, source).expect("admitted source").clone()))
            .collect(),
    ))
}

struct TarArchive<W: Write> {
    writer: NativeNodeGzipWriter<BoundedSink<W>>,
    tar_bytes: usize,
    members: Vec<JsonValue>,
    member_bytes: usize,
}
impl<W: Write> TarArchive<W> {
    /// Open one bounded outer gzip stream without retaining the archive in memory.
    fn new(output: W) -> io::Result<Self> {
        Ok(Self {
            writer: NativeNodeGzipWriter::new(BoundedSink::new(output, MAX_ARCHIVE))?,
            tar_bytes: 0,
            members: Vec::new(),
            member_bytes: 0,
        })
    }
    /// Finish the original two empty tar blocks and exact gzip trailer.
    fn finish(mut self) -> io::Result<()> {
        self.writer.write_all(&[0; 1024])?;
        self.writer.finish()?.inner.flush()
    }

    /// Emit the exact deterministic ustar header, bytes and padding.
    fn append(&mut self, path: &str, bytes: &[u8]) -> io::Result<()> {
        self.member_bytes = self
            .member_bytes
            .checked_add(bytes.len())
            .ok_or_else(|| invalid("archive member count"))?;
        let padding = (512 - bytes.len() % 512) % 512;
        if self.member_bytes > MAX_MEMBERS
            || self
                .tar_bytes
                .checked_add(512 + bytes.len() + padding)
                .is_none_or(|n| n > MAX_TAR)
        {
            return Err(invalid("archive member limit"));
        }
        let name = format!("./{path}");
        let (name, prefix) = if name.len() <= 100 {
            (name.as_str(), "")
        } else {
            let (prefix, name) = name
                .rsplit_once('/')
                .ok_or_else(|| invalid("tar path differs"))?;
            (name, prefix)
        };
        if name.len() > 100 || prefix.len() > 155 {
            return Err(invalid("tar path limit"));
        }
        let mut header = [0u8; 512];
        header[..name.len()].copy_from_slice(name.as_bytes());
        octal(&mut header[100..108], 0o600)?;
        octal(&mut header[108..116], 0)?;
        octal(&mut header[116..124], 0)?;
        octal(&mut header[124..136], bytes.len())?;
        octal(&mut header[136..148], 0)?;
        header[148..156].fill(b' ');
        header[156] = b'0';
        header[257..262].copy_from_slice(b"ustar");
        header[263..265].copy_from_slice(b"00");
        header[345..345 + prefix.len()].copy_from_slice(prefix.as_bytes());
        let checksum: usize = header.iter().map(|byte| usize::from(*byte)).sum();
        header[148..156].copy_from_slice(format!("{checksum:06o}\0 ").as_bytes());
        self.writer.write_all(&header)?;
        self.writer.write_all(bytes)?;
        self.writer.write_all(&[0; 512][..padding])?;
        self.tar_bytes += 512 + bytes.len() + padding;
        self.members.push(object(vec![
            ("path", string(path)),
            ("sha256", string(&sha(bytes))),
            ("sizeBytes", number(bytes.len())),
        ]));
        Ok(())
    }
}

/// Retain only the permitted compressed bytes at either streaming output boundary.
struct BoundedSink<W> {
    inner: W,
    limit: usize,
    count: usize,
}
impl<W> BoundedSink<W> {
    /// Establish the exact source cap before starting compression.
    fn new(inner: W, limit: usize) -> Self {
        Self {
            inner,
            limit,
            count: 0,
        }
    }
}
impl<W: Write> Write for BoundedSink<W> {
    /// Stop before returning any over-limit compressed chunk.
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit - self.count {
            return Err(invalid("compressed export limit"));
        }
        let count = self.inner.write(bytes)?;
        self.count += count;
        Ok(count)
    }
    /// Flush the underlying stream without adding a gzip boundary.
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// Encode the predecessor's null-terminated octal tar fields.
fn octal(field: &mut [u8], value: usize) -> io::Result<()> {
    let encoded = format!("{value:0width$o}", width = field.len() - 1);
    if encoded.len() != field.len() - 1 {
        return Err(invalid("tar integer limit"));
    }
    field[..encoded.len()].copy_from_slice(encoded.as_bytes());
    Ok(())
}
struct ExportValues {
    private: regex::Regex,
}
impl ExportValues {
    /// Reuse one predicate for the existing source payload inspection.
    fn new() -> io::Result<Self> {
        Ok(Self {
            private: regex::Regex::new(
                "(?i)(api[_-]?key|authorization|password|secret|token|https?://)",
            )
            .map_err(io::Error::other)?,
        })
    }
}
impl MaintenanceCanonicalJson for ExportValues {
    fn parse_canonical_object(&mut self, bytes: &[u8]) -> Option<JsonValue> {
        NativeWirePrimitives.parse_canonical_object(bytes)
    }
    fn encode_canonical_json(&mut self, value: &JsonValue) -> Option<Vec<u8>> {
        NativeWirePrimitives.encode_canonical_json(value)
    }
    fn sha256(&mut self, bytes: &[u8]) -> Option<Sha256Digest> {
        MaintenanceCanonicalJson::sha256(&mut NativeWirePrimitives, bytes)
    }
}
impl AdjustmentEvaluationPackageManifestValues for ExportValues {
    type Error = io::Error;
    fn read_bounded_regular_file(
        &mut self,
        root: &str,
        path: &str,
        maximum: u64,
        description: &str,
    ) -> io::Result<Vec<u8>> {
        NativeAdjustmentEvaluationPackageFiles.read_bounded_control(
            root,
            path,
            maximum,
            description,
        )
    }
    fn decode_json(&mut self, bytes: &[u8]) -> io::Result<JsonValue> {
        NativeWirePrimitives
            .decode_json(bytes)
            .ok_or_else(|| invalid("manifest JSON differs"))
    }
    fn sha256(&mut self, bytes: &[u8]) -> Option<Sha256Digest> {
        MaintenanceCanonicalJson::sha256(&mut NativeWirePrimitives, bytes)
    }
    fn utc_epoch_milliseconds(&mut self, value: &JsonValue) -> Option<i64> {
        timestamp(value)
    }
    fn local_date_ordinal(&mut self, date: &str) -> Option<i64> {
        date_ordinal(date)
    }
    fn collect_regular_files(&mut self, root: &str) -> io::Result<Vec<String>> {
        NativeAdjustmentEvaluationPackageFiles.list_regular_files(root)
    }
}
impl AdjustmentEvaluationPackageRowValues for ExportValues {
    fn decode_json(&mut self, bytes: &[u8]) -> Option<JsonValue> {
        NativeWirePrimitives.decode_json(bytes)
    }
    fn js_timestamp_milliseconds(&mut self, value: &JsonValue) -> Option<i64> {
        timestamp(value)
    }
    fn matches_canonical_local_date(&mut self, value: &JsonValue) -> bool {
        matches!(value,JsonValue::String(units) if String::from_utf16(units).is_ok_and(|date|date.len()==10&&date.as_bytes()[4]==b'-'&&date.as_bytes()[7]==b'-'&&date.bytes().enumerate().all(|(i,b)|i==4||i==7||b.is_ascii_digit())))
    }
    fn payload_contains_forbidden_private_material(&mut self, value: &JsonValue) -> bool {
        NativeWirePrimitives
            .encode_ordered_json(value)
            .is_none_or(|bytes| self.private.is_match(&String::from_utf8_lossy(&bytes)))
    }
}
type NativeEvaluationMaterial =
    ForecastEvidenceMaterial<NativeForecastArchiveCodec<fn(&[u8]) -> Option<Vec<u8>>>>;

/// Bind the existing admitted compression and evidence owner.
fn material() -> NativeEvaluationMaterial {
    ForecastEvidenceMaterial::new(NativeForecastArchiveCodec::new(native_node_gzip_candidate))
}
fn timestamp(value: &JsonValue) -> Option<i64> {
    let JsonValue::String(units) = value else {
        return None;
    };
    NativeWirePrimitives.parse_explicit_utc_milliseconds(&String::from_utf16(units).ok()?)
}
fn date_ordinal(date: &str) -> Option<i64> {
    if date.len() != 10 {
        return None;
    }
    NativeWirePrimitives
        .canonical_utc_milliseconds(&format!("{date}T00:00:00.000Z"))
        .map(|ms| ms.div_euclid(86400000))
}
fn integer(value: &JsonValue, key: &str) -> Option<usize> {
    match field(value, key)? {
        JsonValue::Number(n)
            if n.is_finite() && *n >= 0.0 && n.fract() == 0.0 && *n <= usize::MAX as f64 =>
        {
            Some(*n as usize)
        }
        _ => None,
    }
}
fn canonical(value: &JsonValue) -> io::Result<Vec<u8>> {
    NativeWirePrimitives
        .encode_sorted_json(value)
        .ok_or_else(|| invalid("canonical export encoding differs"))
}
fn sha(bytes: &[u8]) -> String {
    hex(&RevisionArchiveCanonicalCodec::sha256(
        &NativeWirePrimitives,
        bytes,
    ))
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn number(value: usize) -> JsonValue {
    JsonValue::Number(value as f64)
}
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}
fn member(name: &str, value: JsonValue) -> (Vec<u16>, JsonValue) {
    (name.encode_utf16().collect(), value)
}
fn object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(name, value)| member(name, value))
            .collect(),
    )
}
fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
