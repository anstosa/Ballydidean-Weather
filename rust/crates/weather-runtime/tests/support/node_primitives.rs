//! Shared bridge to the predecessor's actual JSON, date, base64, and digest primitives.

use std::cell::RefCell;
use std::fmt::Write as _;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use weather_api::{CanonicalInstant, CurrentProjectionError, CurrentTimeResolver};
use weather_contracts::{JsDateOrString, JsonValue};
use weather_database::Sha256Digest;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};
use weather_runtime::{
    MaintenanceAnchorStatusJson, MaintenanceCanonicalJson, NativeShadowProducerPrimitives,
};

/// Use only the actual predecessor primitives from the persistent test bridge.
#[derive(Clone, Copy)]
pub(super) struct NodePrimitives;

impl RevisionArchiveCanonicalCodec for NodePrimitives {
    /// Use the actual predecessor UTC primitive only in this differential fixture.
    fn canonical_utc_milliseconds(&self, text: &str) -> Option<i64> {
        String::from_utf8(primitive("date-parse", text.as_bytes())?)
            .ok()?
            .parse()
            .ok()
    }

    /// Use the original Node hash over exactly the Rust-selected bytes.
    fn sha256(&self, bytes: &[u8]) -> [u8; 32] {
        primitive("sha", bytes)
            .expect("fixture hash")
            .try_into()
            .expect("SHA-256 bytes")
    }
}

impl ArchiveWirePrimitives for NodePrimitives {
    /// Forward UTC output to the real Node primitive in this test.
    fn format_utc_milliseconds(&self, milliseconds: i64) -> Option<String> {
        String::from_utf8(primitive(
            "date-format",
            milliseconds.to_string().as_bytes(),
        )?)
        .ok()
    }

    /// Resolve explicit source offsets through the retained Node date primitive.
    fn parse_explicit_utc_milliseconds(&self, text: &str) -> Option<i64> {
        String::from_utf8(primitive("date-parse-explicit", text.as_bytes())?)
            .ok()?
            .parse()
            .ok()
    }

    /// Convert already parsed Node values through the test-only typed transport.
    fn decode_json(&self, bytes: &[u8]) -> Option<JsonValue> {
        read_value(&primitive("decode", bytes)?)
    }

    /// Use Node's canonical serializer for the exact Rust-constructed value.
    fn encode_sorted_json(&self, value: &JsonValue) -> Option<Vec<u8>> {
        primitive("sorted", &value_bytes(value))
    }

    /// Preserve producer field order through the same real serializer.
    fn encode_ordered_json(&self, value: &JsonValue) -> Option<Vec<u8>> {
        primitive("ordered", &value_bytes(value))
    }

    /// Decode standard base64 through Node in the parity fixture only.
    fn decode_standard_base64(&self, text: &str) -> Option<Vec<u8>> {
        primitive("base64-decode", text.as_bytes())
    }

    /// Encode standard base64 through Node in the parity fixture only.
    fn encode_standard_base64(&self, bytes: &[u8]) -> Option<String> {
        String::from_utf8(primitive("base64-encode", bytes)?).ok()
    }
}

impl NativeShadowProducerPrimitives for NodePrimitives {
    /// Keep legacy wind code unavailable because the oracle uses policy-raw receipts only.
    fn check_legacy_wind_runtime_value(&self, _: &[u16]) -> Option<()> {
        None
    }
}

impl CurrentTimeResolver for NodePrimitives {
    /// Resolve the proof writer's exact fixture UTC values.
    fn resolve(&self, value: &JsDateOrString) -> Result<CanonicalInstant, CurrentProjectionError> {
        let instant = match value {
            JsDateOrString::String(value) => String::from_utf16(value).ok().and_then(|utc| {
                self.canonical_utc_milliseconds(&utc)
                    .map(|epoch_milliseconds| CanonicalInstant {
                        utc,
                        epoch_milliseconds,
                    })
            }),
            JsDateOrString::DateEpochMilliseconds(epoch_milliseconds) => self
                .format_utc_milliseconds(*epoch_milliseconds)
                .map(|utc| CanonicalInstant {
                    utc,
                    epoch_milliseconds: *epoch_milliseconds,
                }),
            JsDateOrString::InvalidDate => None,
        };
        instant.ok_or(CurrentProjectionError {
            code: "fixture_invalid_time",
            detail: "invalid fixture time".to_owned(),
        })
    }
}

impl MaintenanceCanonicalJson for NodePrimitives {
    /// Reuse the same exact predecessor parser for the real proof writer.
    fn parse_canonical_object(&mut self, bytes: &[u8]) -> Option<JsonValue> {
        let value = self.decode_json(bytes)?;
        (self.encode_sorted_json(&value)?.as_slice() == bytes).then_some(value)
    }

    /// Reuse the exact canonical serializer for the proof writer.
    fn encode_canonical_json(&mut self, value: &JsonValue) -> Option<Vec<u8>> {
        self.encode_sorted_json(value)
    }

    /// Bridge only the admitted fixed SHA-256 representation.
    fn sha256(&mut self, bytes: &[u8]) -> Option<Sha256Digest> {
        let digest = RevisionArchiveCanonicalCodec::sha256(self, bytes);
        let mut text = String::with_capacity(64);
        // render the exact fixture hash as the shared lowercase digest
        for byte in digest {
            write!(text, "{byte:02x}").ok()?;
        }
        Some(Sha256Digest::admitted(text))
    }
}

impl MaintenanceAnchorStatusJson for NodePrimitives {
    /// Preserve original JS scalar conversion through the actual fixture primitive.
    fn js_string(&mut self, value: &JsonValue) -> Option<Vec<u16>> {
        let JsonValue::String(units) = read_value(&primitive("string", &value_bytes(value))?)?
        else {
            return None;
        };
        Some(units)
    }
}

/// Locate only the existing test-only Node primitive and predecessor oracle.
pub(super) fn script() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/native-cold-custody-node-oracle.mjs")
}

/// One bounded test-only primitive process, tied to the current test thread.
struct PrimitiveProcess {
    child: Child,
    input: Option<ChildStdin>,
    output: Option<BufReader<ChildStdout>>,
}

impl PrimitiveProcess {
    /// Start the original Node primitive owner once for this disposable test.
    fn start() -> Option<Self> {
        let mut child = Command::new("node")
            .arg(script())
            .arg("bridge")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .ok()?;
        let input = Some(child.stdin.take()?);
        let output = Some(BufReader::new(child.stdout.take()?));
        Some(Self {
            child,
            input,
            output,
        })
    }

    /// Exchange one exact bounded byte request and its real Node response.
    fn request(&mut self, operation: &str, bytes: &[u8]) -> Option<Vec<u8>> {
        let input = self.input.as_mut()?;
        writeln!(input, "{operation} {}", bytes.len()).ok()?;
        input.write_all(bytes).ok()?;
        input.flush().ok()?;
        let output = self.output.as_mut()?;
        let mut header = String::new();
        output.read_line(&mut header).ok()?;
        let (status, length) = header.trim_end().split_once(' ')?;
        let length: usize = length.parse().ok()?;
        // enforce the bounded fixture response before allocation
        if status != "ok" || length > 32 * 1024 * 1024 {
            return None;
        }
        let mut bytes = vec![0; length];
        output.read_exact(&mut bytes).ok()?;
        Some(bytes)
    }
}

impl Drop for PrimitiveProcess {
    /// Close only the fixture streams and reap its child at test-thread exit.
    fn drop(&mut self) {
        self.input.take();
        self.output.take();
        let _exit = self.child.wait();
    }
}

thread_local! {
    static PRIMITIVE_PROCESS: RefCell<Option<PrimitiveProcess>> = const { RefCell::new(None) };
}

/// Drive only the actual Node primitives without a per-field process launch.
pub(super) fn primitive(operation: &str, bytes: &[u8]) -> Option<Vec<u8>> {
    PRIMITIVE_PROCESS.with(|process| {
        let mut process = process.borrow_mut();
        // bind the disposable helper lazily and retain it through this test
        if process.is_none() {
            *process = Some(PrimitiveProcess::start()?);
        }
        process.as_mut()?.request(operation, bytes)
    })
}

/// Decode only the test bridge's typed-value records, not JSON syntax.
pub(super) fn read_value(bytes: &[u8]) -> Option<JsonValue> {
    let text = std::str::from_utf8(bytes).ok()?;
    let mut lines = text.lines();
    let result = read_tree(&mut lines)?;
    lines.next().is_none().then_some(result)
}

/// Read one typed fixture record and its announced children.
fn read_tree(lines: &mut std::str::Lines<'_>) -> Option<JsonValue> {
    let line = lines.next()?;
    let (tag, value) = line.split_at_checked(1)?;
    Some(match tag {
        "N" => JsonValue::Null,
        "B" => JsonValue::Boolean(value == "1"),
        "D" => JsonValue::Number(value.parse().ok()?),
        "S" => JsonValue::String(text_units(value)?),
        "A" => {
            let count: usize = value.parse().ok()?;
            JsonValue::Array(
                (0..count)
                    .map(|_| read_tree(lines))
                    .collect::<Option<Vec<_>>>()?,
            )
        }
        "O" => {
            let count: usize = value.parse().ok()?;
            let fields = (0..count)
                .map(|_| {
                    let key = text_units(lines.next()?.strip_prefix('K')?)?;
                    Some((key, read_tree(lines)?))
                })
                .collect::<Option<Vec<_>>>()?;
            JsonValue::Object(fields)
        }
        _ => return None,
    })
}

/// Decode UTF-16 test transport units without replacing surrogate code units.
fn text_units(value: &str) -> Option<Vec<u16>> {
    value
        .as_bytes()
        .chunks(4)
        .map(|chunk| u16::from_str_radix(std::str::from_utf8(chunk).ok()?, 16).ok())
        .collect()
}

/// Encode generated JSON values into the test-only primitive bridge.
pub(super) fn value_bytes(value: &JsonValue) -> Vec<u8> {
    let mut text = String::new();
    write_tree(value, &mut text);
    text.into_bytes()
}

/// Emit typed records instead of implementing JSON formatting in the fixture.
fn write_tree(value: &JsonValue, output: &mut String) {
    match value {
        JsonValue::Null => output.push_str("N\n"),
        JsonValue::Boolean(value) => output.push_str(if *value { "B1\n" } else { "B0\n" }),
        JsonValue::Number(value) => writeln!(output, "D{value}").unwrap(),
        JsonValue::String(value) => write_units('S', value, output),
        JsonValue::Array(values) => {
            writeln!(output, "A{}", values.len()).unwrap();
            // emit each announced member in order
            for value in values {
                write_tree(value, output);
            }
        }
        JsonValue::Object(fields) => {
            writeln!(output, "O{}", fields.len()).unwrap();
            // preserve the source field order for the actual Node serializer
            for (key, value) in fields {
                write_units('K', key, output);
                write_tree(value, output);
            }
        }
    }
}

/// Emit one fixture UTF-16 string or object key.
fn write_units(tag: char, units: &[u16], output: &mut String) {
    output.push(tag);
    // preserve every code unit without invoking a JSON encoder
    for unit in units {
        write!(output, "{unit:04x}").unwrap();
    }
    output.push('\n');
}
