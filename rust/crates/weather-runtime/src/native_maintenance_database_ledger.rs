//! Exact rolling database-ledger receipt for the maintenance workstation reader.

use crate::{
    NativeWirePrimitives, VerifiedNativeMaintenanceDatabaseLedger,
    capture_epoch_witness::resolve_operator_capture_clock,
    maintenance_canonical_json::{exact_keys, field, text, valid_hash},
};
use std::{
    fs,
    io::{self, Read, Write},
    os::unix::fs::{FileTypeExt, MetadataExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    thread,
};
use weather_contracts::JsonValue;
use weather_edge::ArchiveWirePrimitives;

const LEDGER_VERSION: &str = "adjustment-database-ledger/v3";
const OUTPUT_MAXIMUM: usize = 4_832 * 1_024 + 1;
const DIAGNOSTIC_MAXIMUM: usize = 16 * 1_024;
const LEDGER_FIELDS: &[&str] = &["contractVersion", "databaseManifest", "snapshotAt"];
const MANIFEST_FIELDS: &[&str] = &[
    "contract_version",
    "migration_checksums",
    "migration_history_sha256",
    "migration_names",
    "query_contract_sha256",
    "query_contract_version",
    "row_schema_sha256",
    "schema_migration",
    "site_key",
    "site_timezone",
];
const EXPECTED: &[(usize, &str)] = &[
    (
        21,
        "6de5c8c7efaa448aeb12bf1a9debe6fe7d4d4d1003ee0e21ab619ffa624c3424",
    ),
    (
        22,
        "422dac5e0547295b7fd973f8e1b234a013dd629b04001215a044bee928b77b6e",
    ),
    (
        23,
        "c8b43b714eb555027ea7439a813831ab7326ad951b800362f35b2343330da81f",
    ),
];

/// Admit the exact source line framing, rolling schema identity, and server clock.
pub fn read_native_maintenance_database_ledger_bytes(
    bytes: &[u8],
) -> io::Result<VerifiedNativeMaintenanceDatabaseLedger> {
    // retain the original full-ledger transport bound and producer-order JSON frame
    if bytes.is_empty() || bytes.len() > OUTPUT_MAXIMUM {
        return Err(invalid_ledger());
    }
    let value = NativeWirePrimitives
        .decode_json(bytes)
        .ok_or_else(invalid_ledger)?;
    // reject private payloads and any different serialization at the transport boundary
    if NativeWirePrimitives.encode_ordered_json(&value).as_deref() != Some(bytes)
        || !exact_keys(&value, LEDGER_FIELDS)
        || text(&value, "contractVersion").as_deref() != Some(LEDGER_VERSION)
    {
        return Err(invalid_ledger());
    }
    let manifest = field(&value, "databaseManifest").ok_or_else(invalid_ledger)?;
    let migration_history_sha256 = validate_manifest(manifest)?;
    let snapshot_at = text(&value, "snapshotAt").ok_or_else(invalid_ledger)?;
    // require the original canonical UTC millisecond clock
    if resolve_operator_capture_clock(&snapshot_at, &NativeWirePrimitives).is_none() {
        return Err(invalid_ledger());
    }
    Ok(VerifiedNativeMaintenanceDatabaseLedger {
        contract_version: LEDGER_VERSION.to_owned(),
        migration_history_sha256,
        snapshot_at,
    })
}

/// Project the actual root read-only SQL export envelope to the original canonical ledger bytes.
pub fn project_native_maintenance_database_ledger_v3(input: impl Read) -> io::Result<Vec<u8>> {
    const TRANSACTION_KEYS: &[&str] = &[
        "created_at_utc",
        "idle_in_transaction_session_timeout",
        "isolation_level",
        "lock_timeout",
        "read_only",
        "statement_timeout",
    ];
    let bytes = bounded_pipe(input, 64 * 1024)?;
    let wire = NativeWirePrimitives;
    let value = wire.decode_json(&bytes).ok_or_else(invalid_ledger)?;
    // admit the exact metadata-only SQL function frame without private payloads
    if !exact_keys(&value, &["databaseManifest", "payload", "transaction"])
        || !matches!(field(&value, "payload"), Some(JsonValue::Null))
    {
        return Err(invalid_ledger());
    }
    let manifest = field(&value, "databaseManifest").ok_or_else(invalid_ledger)?;
    validate_manifest(manifest)?;
    let transaction = field(&value, "transaction").ok_or_else(invalid_ledger)?;
    // bind the same fixed read-only transaction controls before releasing the server clock
    if !exact_keys(transaction, TRANSACTION_KEYS)
        || [
            ("idle_in_transaction_session_timeout", "30s"),
            ("isolation_level", "repeatable read"),
            ("lock_timeout", "5s"),
            ("read_only", "on"),
            ("statement_timeout", "5min"),
        ]
        .iter()
        .any(|(key, expected)| text(transaction, key).as_deref() != Some(*expected))
    {
        return Err(invalid_ledger());
    }
    let created = text(transaction, "created_at_utc").ok_or_else(invalid_ledger)?;
    let millis = wire
        .parse_explicit_utc_milliseconds(&created)
        .ok_or_else(invalid_ledger)?;
    let snapshot = wire
        .format_utc_milliseconds(millis)
        .ok_or_else(invalid_ledger)?;
    let projector = JsonValue::Object(vec![
        (
            "contractVersion".encode_utf16().collect(),
            JsonValue::String(LEDGER_VERSION.encode_utf16().collect()),
        ),
        (
            "databaseManifest".encode_utf16().collect(),
            manifest.clone(),
        ),
        (
            "snapshotAt".encode_utf16().collect(),
            JsonValue::String(snapshot.encode_utf16().collect()),
        ),
    ]);
    wire.encode_sorted_json(&projector)
        .ok_or_else(invalid_ledger)
}

/// Require one exact rolling ledger and the unchanged schema/query/site contract.
fn validate_manifest(manifest: &JsonValue) -> io::Result<String> {
    // match every fixed database export identity before admitting any of the three ledgers
    if !exact_keys(manifest, MANIFEST_FIELDS)
        || [
            (
                "contract_version",
                "adjustment-evaluation-export-manifest/v1",
            ),
            ("schema_migration", "0017_adjustment_evaluation_export.sql"),
            (
                "query_contract_version",
                "adjustment-evaluation-export-query/v1",
            ),
            ("site_key", "ballydidean"),
            ("site_timezone", "America/Los_Angeles"),
            (
                "row_schema_sha256",
                "c21782034130d003e4af48fada4c8f6545f6f1c4dff550f92d0523c7d96d20e2",
            ),
            (
                "query_contract_sha256",
                "c860039c72818a9b813ed9f9d93f5d8e115f4144e99d698734e9159ca3457bd4",
            ),
        ]
        .iter()
        .any(|(key, expected)| text(manifest, key).as_deref() != Some(*expected))
    {
        return Err(invalid_ledger());
    }
    let history = text(manifest, "migration_history_sha256").ok_or_else(invalid_ledger)?;
    let count = EXPECTED
        .iter()
        .find(|(_, digest)| *digest == history)
        .map(|(n, _)| *n)
        .ok_or_else(invalid_ledger)?;
    let names = array_strings(manifest, "migration_names")?;
    let checksums = array_strings(manifest, "migration_checksums")?;
    // preserve the exact population and unambiguous name:checksum ledger grammar
    if names.len() != count || checksums.len() != count {
        return Err(invalid_ledger());
    }
    let mut preimage = Vec::new();
    // bind the complete ordered reviewed migration bytes, not merely the last name
    for (index, (name, checksum)) in names.iter().zip(checksums).enumerate() {
        let prefix = format!("{:04}_", index + 1);
        let suffix = name
            .strip_prefix(&prefix)
            .and_then(|name| name.strip_suffix(".sql"));
        // require source migration shape so serialized delimiters cannot be ambiguous
        if suffix.is_none_or(|value| {
            value.is_empty()
                || !value
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
        }) || !valid_hash(&checksum)
        {
            return Err(invalid_ledger());
        }
        // append exactly the original separator between complete ledger entries
        if index != 0 {
            preimage.push(b'\n');
        }
        preimage.extend_from_slice(name.as_bytes());
        preimage.push(b':');
        preimage.extend_from_slice(checksum.as_bytes());
    }
    let actual =
        weather_edge::RevisionArchiveCanonicalCodec::sha256(&NativeWirePrimitives, &preimage);
    // admit only the frozen full source history for this exact rolling prefix
    if actual != parse_hash(&history).ok_or_else(invalid_ledger)? {
        return Err(invalid_ledger());
    }
    Ok(history)
}

/// Decode only actual string array members from the retained manifest.
fn array_strings(value: &JsonValue, key: &str) -> io::Result<Vec<String>> {
    let Some(JsonValue::Array(items)) = field(value, key) else {
        return Err(invalid_ledger());
    };
    items
        .iter()
        .map(|value| match value {
            JsonValue::String(units) => String::from_utf16(units).map_err(|_| invalid_ledger()),
            _ => Err(invalid_ledger()),
        })
        .collect()
}

/// Parse an exact already-admitted lowercase SHA-256 string.
fn parse_hash(value: &str) -> Option<[u8; 32]> {
    let digest = weather_edge::RevisionGapSha256::from_lower_hex(value)?;
    Some(*digest.as_bytes())
}

/// Reuse the fixed original forced-command account and independent owner socket.
pub struct NativeMaintenanceDatabaseLedgerReader {
    ssh_config: PathBuf,
}

impl NativeMaintenanceDatabaseLedgerReader {
    /// Bind the source SSH config to the already verified installed release root.
    pub fn new(verified_installed_release_root: &Path) -> Self {
        Self {
            ssh_config: verified_installed_release_root.join("deploy/config/ssh_config.example"),
        }
    }

    /// Read one genuine metadata-only ledger from the original remote command.
    pub fn fetch(&self) -> io::Result<VerifiedNativeMaintenanceDatabaseLedger> {
        let bytes = self.read_document("adjustment-database-ledger-v3", &[])?;
        read_native_maintenance_database_ledger_bytes(&bytes)
    }

    /// Reuse the source forced transport only through the closed typed caller methods.
    pub(crate) fn read_document(&self, operation: &str, arguments: &[&str]) -> io::Result<Vec<u8>> {
        self.request_document(operation, arguments, None)
    }

    /// Send one bounded typed source operation with optional exact canonical stdin.
    pub(crate) fn request_document(
        &self,
        operation: &str,
        arguments: &[&str],
        body: Option<&[u8]>,
    ) -> io::Result<Vec<u8>> {
        // retain the original 128 KiB scorecard bound without widening other typed operations
        let body_maximum = if operation == "install-adjustment-scorecard-v2" {
            128 * 1024
        } else {
            64 * 1024
        };
        if body.is_some_and(|bytes| bytes.len() > body_maximum) {
            return Err(invalid_ledger());
        }
        let agent = owner_agent_socket()?;
        // retain the exact ordinary remote command without shell interpolation
        let mut child = Command::new("/usr/bin/ssh")
            .arg("-F")
            .arg(&self.ssh_config)
            .args([
                "-o",
                "BatchMode=yes",
                "-o",
                "ConnectTimeout=15",
                "--",
                "weather-pi",
            ])
            .arg(operation)
            .args(arguments)
            .env_clear()
            .env("HOME", "/home/ubuntu")
            .env("LANG", "C")
            .env("LC_ALL", "C")
            .env("PATH", "/usr/bin:/bin")
            .env("SSH_AUTH_SOCK", agent)
            .stdin(if body.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let input = child.stdin.take();
        let output = child.stdout.take().ok_or_else(invalid_ledger)?;
        let diagnostics = child.stderr.take().ok_or_else(invalid_ledger)?;
        // consume both streams independently and stop a child exceeding either source bound
        thread::scope(|scope| -> io::Result<Vec<u8>> {
            let (send, receive) = mpsc::channel();
            let output_send = send.clone();
            let inputs = usize::from(body.is_some());
            // write a canonical request concurrently with both bounded response pipes
            if let Some(body) = body {
                let input_send = send.clone();
                scope.spawn(move || {
                    let result = input
                        .ok_or_else(invalid_ledger)
                        .and_then(|mut input| input.write_all(body))
                        .map(|()| Vec::new());
                    let _ = input_send.send((false, result));
                });
            }
            scope.spawn(move || {
                let _ = output_send.send((true, bounded_pipe(output, OUTPUT_MAXIMUM)));
            });
            scope.spawn(move || {
                let _ = send.send((false, bounded_pipe(diagnostics, DIAGNOSTIC_MAXIMUM)));
            });
            let mut bytes = None;
            let mut failed = false;
            // wait for both owning readers, killing only this child on a refused response
            for _ in 0..2 + inputs {
                match receive.recv() {
                    Ok((true, Ok(value))) => bytes = Some(value),
                    Ok((false, Ok(_))) => {}
                    _ => {
                        failed = true;
                        let _ = child.kill();
                    }
                }
            }
            let status = child.wait()?;
            // report neither diagnostic material nor an incomplete transport result
            if failed || !status.success() {
                return Err(invalid_ledger());
            }
            bytes.ok_or_else(invalid_ledger)
        })
    }
}

impl crate::NativeMaintenanceCaptureRemoteCommandPort for NativeMaintenanceDatabaseLedgerReader {
    type Error = io::Error;

    /// Dispatch only a closed forced read verb and its exact fixed operands.
    fn read_forced(
        &mut self,
        operation: crate::NativeMaintenanceCaptureRemoteOperation,
    ) -> io::Result<Vec<u8>> {
        self.read_document(operation.verb(), operation.arguments())
    }
}

/// Read no more than one byte past each source response ceiling.
fn bounded_pipe(mut pipe: impl Read, limit: usize) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    pipe.by_ref()
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)?;
    // reject oversize responses rather than admitting a truncated line
    if bytes.len() > limit {
        return Err(invalid_ledger());
    }
    Ok(bytes)
}

/// Admit only the literal current-user inherited source socket.
fn owner_agent_socket() -> io::Result<PathBuf> {
    let path = PathBuf::from(std::env::var_os("SSH_AUTH_SOCK").ok_or_else(invalid_ledger)?);
    let details = fs::symlink_metadata(&path)?;
    // safety: getuid has no preconditions or memory effects
    let uid = unsafe { libc::getuid() };
    // retain only one actual current-user socket and the source path bound
    if !path.is_absolute()
        || path.as_os_str().len() > 4096
        || !details.file_type().is_socket()
        || details.uid() != uid
        || path.canonicalize()?.as_os_str() != path.as_os_str()
    {
        return Err(invalid_ledger());
    }
    Ok(path)
}

/// Keep this private ledger transport failure value-free.
fn invalid_ledger() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "maintenance database ledger unavailable",
    )
}

#[cfg(test)]
#[path = "native_maintenance_database_ledger_test.rs"]
mod tests;

impl crate::NativeMaintenanceGapRemotePort for NativeMaintenanceDatabaseLedgerReader {
    type Error = io::Error;

    /// Obtain the actual original compact permanent-gap frontier.
    fn fetch_start(&mut self) -> io::Result<Vec<u8>> {
        self.read_document("adjustment-revision-gap-start-v1", &[])
    }

    /// Request one page only under the admitted original source cursor.
    fn fetch_page(&mut self, start: weather_edge::RevisionGapTransferStart) -> io::Result<Vec<u8>> {
        let start_hash = crate::native_maintenance_retirement_files_v2::format_digest(
            *start.start_sha256.as_bytes(),
        );
        let frontier = crate::native_maintenance_retirement_files_v2::format_digest(
            *start.frontier_sha256.as_bytes(),
        );
        self.read_document("adjustment-revision-gap-page-v1", &[&start_hash, &frontier])
    }

    /// Send only the original exact page and previously durably verified graph identity.
    fn acknowledge(
        &mut self,
        page: weather_edge::RevisionGapSha256,
        graph: [u8; 32],
    ) -> io::Result<Vec<u8>> {
        let page = crate::native_maintenance_retirement_files_v2::format_digest(*page.as_bytes());
        let graph = crate::native_maintenance_retirement_files_v2::format_digest(graph);
        self.read_document("adjustment-revision-gap-ack-v1", &[&page, &graph])
    }
}

impl crate::NativeRegistrationLifecycleRemoteOperationPort
    for NativeMaintenanceDatabaseLedgerReader
{
    type Error = io::Error;

    /// Apply only the original typed lifecycle read or canonical bootstrap request.
    fn execute(
        &mut self,
        operation: crate::NativeRegistrationLifecycleRemoteOperation,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<u8>, Self::Error>> + '_>>
    {
        Box::pin(async move {
            self.request_document(
                operation.command(),
                &operation.arguments(),
                operation.body(),
            )
        })
    }
}
