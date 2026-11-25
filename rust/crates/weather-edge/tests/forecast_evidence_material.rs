use std::cell::Cell;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use weather_contracts::JsonValue;
use weather_edge::ForecastEvidenceReceiptCodec;
use weather_edge::{
    ForecastCaptureChannel, ForecastCaptureSource, ForecastCaptureVersion,
    ForecastEvidenceMaterial, ForecastEvidenceMaterialCodec, ForecastEvidenceMaterialError,
    ForecastEvidenceObjectVersion, ForecastEvidenceWindow, RevisionArchiveCanonicalCodec,
};

const NOW: &str = "2026-10-07T08:54:05.500Z";

/// run the actual Node canonical and compression source only inside a disposable test.
fn node(mode: &[&str], input: &[u8]) -> Vec<u8> {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/forecast-evidence-material-node-oracle.mjs");
    let mut child = Command::new("node")
        .arg(script)
        .args(mode)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    result.stdout
}

/// decode the test's hex framing without interpreting application JSON.
fn from_hex(bytes: &[u8]) -> Vec<u8> {
    bytes
        .chunks_exact(2)
        .map(
            // retain each byte of the original bounded oracle record
            |pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap(),
        )
        .collect()
}

/// recover a generated typed value from a test-only line frame.
fn read_typed<'a>(lines: &mut impl Iterator<Item = &'a str>) -> JsonValue {
    let (kind, text) = lines.next().unwrap().split_at(1);
    match kind {
        "n" => JsonValue::Null,
        "t" => JsonValue::Boolean(true),
        "f" => JsonValue::Boolean(false),
        "d" => JsonValue::Number(text.parse().unwrap()),
        "s" => JsonValue::String(
            text.split(',')
                .filter(|part| !part.is_empty())
                .map(|part| part.parse().unwrap())
                .collect(),
        ),
        "a" => JsonValue::Array(
            (0..text.parse::<usize>().unwrap())
                .map(|_| read_typed(lines))
                .collect(),
        ),
        "o" => JsonValue::Object(
            (0..text.parse::<usize>().unwrap())
                .map(
                    // retain original ECMAScript property labels and their structured values
                    |_| {
                        let JsonValue::String(key) = read_typed(lines) else {
                            panic!("expected a fixture field key")
                        };
                        (key, read_typed(lines))
                    },
                )
                .collect(),
        ),
        _ => panic!("unexpected witness kind {kind}"),
    }
}

/// encode only the test's raw typed string frame.
fn write_string(value: &[u16], output: &mut String) {
    output.push('s');
    // retain the original UTF-16 units
    for (index, unit) in value.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        write!(output, "{unit}").unwrap();
    }
    output.push('\n');
}

/// send the actual Rust logical structure to the independent Node canonical codec.
fn write_typed(value: &JsonValue, output: &mut String) {
    match value {
        JsonValue::Null => output.push_str("n\n"),
        JsonValue::Boolean(true) => output.push_str("t\n"),
        JsonValue::Boolean(false) => output.push_str("f\n"),
        JsonValue::Number(number) => writeln!(output, "d{number}").unwrap(),
        JsonValue::String(value) => write_string(value, output),
        JsonValue::Array(values) => {
            writeln!(output, "a{}", values.len()).unwrap();
            // retain semantic array order
            for value in values {
                write_typed(value, output);
            }
        }
        JsonValue::Object(fields) => {
            writeln!(output, "o{}", fields.len()).unwrap();
            // retain the actual projected names and values
            for (key, value) in fields {
                write_string(key, output);
                write_typed(value, output);
            }
        }
    }
}

struct Fixture {
    fields: BTreeMap<String, Vec<u8>>,
    forecast: JsonValue,
    times: BTreeMap<String, i64>,
}

impl Fixture {
    /// obtain expected data from the original deployed functions and normal existing fixtures.
    fn load(name: &str) -> Self {
        let output = String::from_utf8(node(&["fixture", name], b"")).unwrap();
        let fields: BTreeMap<String, Vec<u8>> = output
            .lines()
            .map(
                // decode the framing and retain the complete original field bytes
                |line| {
                    let (key, value) = line.split_once('=').unwrap();
                    (key.to_owned(), from_hex(value.as_bytes()))
                },
            )
            .collect();
        let mut typed = std::str::from_utf8(&fields["typed"]).unwrap().lines();
        let forecast = read_typed(&mut typed);
        assert!(typed.next().is_none());
        let times = std::str::from_utf8(&fields["clocks"])
            .unwrap()
            .lines()
            .map(
                // preserve each independently parsed source timestamp
                |line| {
                    let (key, value) = line.split_once('=').unwrap();
                    (key.to_owned(), value.parse().unwrap())
                },
            )
            .collect();
        Self {
            fields,
            forecast,
            times,
        }
    }
}

struct Codec<'a> {
    fixture: &'a Fixture,
    gzip_calls: &'a Cell<usize>,
}

impl RevisionArchiveCanonicalCodec for Codec<'_> {
    /// validate the exact independent fixture timestamps without introducing a Rust calendar.
    fn canonical_utc_milliseconds(&self, value: &str) -> Option<i64> {
        self.fixture.times.get(value).copied()
    }

    /// hash the bytes actually passed by the Rust assembler.
    fn sha256(&self, bytes: &[u8]) -> [u8; 32] {
        from_hex(&node(&["hash"], bytes)).try_into().unwrap()
    }
}

impl ForecastEvidenceMaterialCodec for Codec<'_> {
    /// deserialize only the actual expected producer input in this test.
    fn decode_projected_forecast(&self, bytes: &[u8]) -> Option<JsonValue> {
        (bytes == self.fixture.fields["input"]).then(|| self.fixture.forecast.clone())
    }

    /// delegate canonical JSON bytes to the original Node implementation.
    fn canonical_json(&self, value: &JsonValue) -> Option<Vec<u8>> {
        let mut input = String::new();
        write_typed(value, &mut input);
        Some(node(&["canonical"], input.as_bytes()))
    }

    /// instrument actual independently executed gzip without changing it.
    fn gzip_level_nine_zero_mtime(&self, bytes: &[u8]) -> Option<Vec<u8>> {
        self.gzip_calls.set(self.gzip_calls.get() + 1);
        Some(node(&["gzip"], bytes))
    }

    /// delegate exact standard base64 formatting to the test's existing Node codec.
    fn standard_base64(&self, bytes: &[u8]) -> Option<String> {
        String::from_utf8(node(&["base64"], bytes)).ok()
    }
}

impl ForecastEvidenceReceiptCodec for Codec<'_> {
    /// decode the actual retained witness bytes through Node's existing JSON implementation.
    fn decode_retained_json(&self, bytes: &[u8]) -> Option<JsonValue> {
        let encoded = String::from_utf8(node(&["describe"], bytes)).ok()?;
        let mut lines = encoded.lines();
        let value = read_typed(&mut lines);
        lines.next().is_none().then_some(value)
    }

    /// delegate the exact standard base64 spelling to the Node test codec.
    fn decode_standard_base64(&self, value: &str) -> Option<Vec<u8>> {
        Some(node(&["base64-decode"], value.as_bytes()))
    }

    /// delegate bounded gzip reading to the existing Node fixture.
    fn gunzip_bounded(&self, bytes: &[u8], maximum_bytes: usize) -> Option<Vec<u8>> {
        Some(node(&["gunzip", &maximum_bytes.to_string()], bytes))
    }
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native {
    use std::fs::{self, DirBuilder};
    use std::os::unix::fs::DirBuilderExt;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use weather_edge::{
        ForecastAllocationStatfsPort, ForecastAllocationStatfsSample, ForecastEvidenceMaterial,
        ForecastObjectCommitInput, ForecastObjectCommitOutcome, NativeForecastObjectAllocation,
        NativeForecastObjectStore, PreparedForecastEvidence,
    };

    use std::cell::Cell;
    use weather_edge::{
        AdmittedForecastScheduleCapture, CompletedForecastResponseCapture,
        ForecastArchiveAppendOutcome, ForecastEvidenceOutcome, ForecastResponseTerminal,
        ForecastScheduleArchive, ForecastScheduledEvidenceArchive, NativeForecastCaptureStore,
        PublicServingApplicationBody, PublicServingApplicationHeaders,
        PublicServingApplicationResponse, PublicServingHeaderProfile,
        RevisionArchiveCanonicalClock, observe_forecast_response_terminal,
        persist_forecast_response_capture, prepare_forecast_response_capture,
    };

    use super::{Codec, Fixture, NOW};

    struct PrivateFixture(PathBuf);

    impl PrivateFixture {
        /// allocate only a fresh disposable root and four fixed native evidence directories.
        fn create() -> Self {
            let temporary = fs::canonicalize(std::env::temp_dir()).unwrap();
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = temporary.join(format!(
                "weather-forecast-object-fixture-{}-{nonce}",
                std::process::id()
            ));
            DirBuilder::new().mode(0o700).create(&root).unwrap();
            // match the existing owner-private destination layout
            for child in ["objects", "receipts", "channels", "channel-bindings"] {
                DirBuilder::new()
                    .mode(0o700)
                    .create(root.join(child))
                    .unwrap();
            }
            Self(root)
        }
    }

    impl Drop for PrivateFixture {
        /// clean up only the exact root created by this test.
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    struct Admission(usize);

    impl ForecastAllocationStatfsPort for Admission {
        /// confirm the operating system boundary uses only the fresh disposable root.
        fn approved_sample(
            &mut self,
            root: &Path,
        ) -> std::io::Result<ForecastAllocationStatfsSample> {
            assert!(
                root.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("weather-forecast-object-fixture-")
            );
            self.0 += 1;
            Ok(ForecastAllocationStatfsSample {
                available_blocks: 600_000,
                block_size: 4_096,
                free_inodes: 65_536,
            })
        }
    }

    /// publish actual assembled material and prove exact retained bytes against the Node producer.
    pub(super) fn verify(
        material: &ForecastEvidenceMaterial<Codec<'_>>,
        prepared: &PreparedForecastEvidence,
        fixture: &Fixture,
    ) {
        let root = PrivateFixture::create();
        let mut storage = NativeForecastObjectStore::new(root.0.clone()).unwrap();
        let mut admission =
            NativeForecastObjectAllocation::new(root.0.clone(), Admission(0)).unwrap();
        let fresh = storage
            .commit(
                ForecastObjectCommitInput {
                    compressed_object: prepared.compressed_object(),
                    first_edge_committed_at: NOW,
                    prepared,
                },
                material,
                &mut admission,
            )
            .unwrap();
        assert_eq!(
            fresh,
            ForecastObjectCommitOutcome::Created {
                object_created: true
            }
        );
        let retry = storage
            .commit(
                ForecastObjectCommitInput {
                    compressed_object: prepared.compressed_object(),
                    first_edge_committed_at: "2026-10-07T08:55:00.000Z",
                    prepared,
                },
                material,
                &mut admission,
            )
            .unwrap();
        assert_eq!(retry, ForecastObjectCommitOutcome::Duplicate);
        admission.refresh_from_disk().unwrap();
        let accounting = admission.census();
        assert_eq!(accounting.object_count, 1);
        assert_eq!(accounting.receipt_count, 1);
        assert_eq!(accounting.channel_assertion_count, 0);
        assert_eq!(admission.into_parts().1.0, 1);
        let receipt = root.0.join("receipts").join(format!(
            "sha256-{}.json",
            std::str::from_utf8(&fixture.fields["identity"]).unwrap(),
        ));
        let object = root.0.join("objects").join(format!(
            "sha256-{}.json.gz",
            std::str::from_utf8(&fixture.fields["object"]).unwrap(),
        ));
        assert_eq!(fs::read(receipt).unwrap(), fixture.fields["receipt"]);
        assert_eq!(fs::read(object).unwrap(), fixture.fields["gzip"]);
    }
    struct Clock(&'static str);

    impl RevisionArchiveCanonicalClock for Clock {
        /// use the original independently known UTC fixture instant.
        fn canonical_now(&mut self) -> Option<String> {
            Some(self.0.to_owned())
        }
    }

    struct NoArchive;

    impl ForecastScheduledEvidenceArchive<PreparedForecastEvidence> for NoArchive {
        type Error = ();

        /// this normal configured fixture leaves further scheduler transfer inactive.
        fn append_committed(
            &mut self,
            _: &str,
            _: &PreparedForecastEvidence,
        ) -> Result<ForecastArchiveAppendOutcome, Self::Error> {
            unreachable!("fixture archive transfer is reset")
        }
    }

    /// deliver one normal typed public GET event through the actual native evidence owner.
    fn observe(
        store: &mut NativeForecastCaptureStore<Codec<'_>, Admission>,
        fixture: &Fixture,
        channel_bound: bool,
        scheduled: bool,
        clock: &mut Clock,
    ) -> CompletedForecastResponseCapture {
        let response = PublicServingApplicationResponse {
            body: PublicServingApplicationBody::Bytes(Some(fixture.fields["input"].clone())),
            header_profile: PublicServingHeaderProfile::Standard,
            headers: PublicServingApplicationHeaders {
                allow: None,
                cache_control: "no-store".to_owned(),
                content_length: fixture.fields["input"].len(),
                content_type: "application/json; charset=utf-8".to_owned(),
                vary: Some("Accept"),
            },
            status_code: 200,
            track_successful_forecast_response: true,
        };
        let before = store.status();
        let mut pending = prepare_forecast_response_capture(
            &response,
            Some(&fixture.forecast),
            &[("days", "1")],
            channel_bound,
            scheduled.then(|| {
                AdmittedForecastScheduleCapture::admitted("capture/2026-10-07T08:50:00.000Z")
            }),
            store,
        );
        let work = observe_forecast_response_terminal(
            &mut pending,
            ForecastResponseTerminal::Finished,
            clock,
            store,
        )
        .unwrap();
        assert_eq!(store.status(), before);
        persist_forecast_response_capture(
            work,
            store,
            ForecastScheduleArchive::<NoArchive>::Disabled,
        )
    }

    /// compare actual native first-channel bytes and restart catalog against the original Node store.
    #[test]
    fn composed_capture_store_matches_first_channel_and_snapshot_after_restart() {
        for (case, legacy) in [("channel-public", false), ("channel-legacy", true)] {
            let fixture = Fixture::load(case);
            let gzip_calls = Cell::new(0);
            let root = PrivateFixture::create();
            let mut store = NativeForecastCaptureStore::initialize(
                root.0.clone(),
                Codec {
                    fixture: &fixture,
                    gzip_calls: &gzip_calls,
                },
                Admission(0),
                &mut Clock(NOW),
            );
            assert!(store.initialized(), "{case}");
            assert_eq!(
                observe(&mut store, &fixture, !legacy, false, &mut Clock(NOW)).evidence,
                ForecastEvidenceOutcome::Created,
            );
            let repeated = observe(
                &mut store,
                &fixture,
                true,
                true,
                &mut Clock("2026-10-07T08:55:00.000Z"),
            );
            assert_eq!(repeated.evidence, ForecastEvidenceOutcome::Duplicate);
            assert_eq!(
                repeated.scheduled.unwrap().settlement.wire_value(),
                "duplicate"
            );
            let status = store.status();
            assert_eq!(
                status.objects.to_string(),
                std::str::from_utf8(&fixture.fields["archive_objects"]).unwrap()
            );
            assert_eq!(
                status.channel_assertions.to_string(),
                std::str::from_utf8(&fixture.fields["archive_channels"]).unwrap()
            );
            assert_eq!(
                status.finished_successes.to_string(),
                std::str::from_utf8(&fixture.fields["archive_finished"]).unwrap()
            );
            assert_eq!(
                status.provenance_incomplete.to_string(),
                std::str::from_utf8(&fixture.fields["archive_incomplete"]).unwrap()
            );
            let expected_binding = root.0.join("channel-bindings").join(format!(
                "sha256-{}.json",
                std::str::from_utf8(&fixture.fields["identity"]).unwrap(),
            ));
            let expected_assertion = root.0.join("channels").join(format!(
                "sha256-{}.json",
                std::str::from_utf8(&fixture.fields["assertion_address"]).unwrap(),
            ));
            assert_eq!(
                fs::read(expected_binding).unwrap(),
                fixture.fields["binding"]
            );
            assert_eq!(
                fs::read(expected_assertion).unwrap(),
                fixture.fields["assertion"]
            );
            drop(store);
            let reopened = NativeForecastCaptureStore::initialize(
                root.0.clone(),
                Codec {
                    fixture: &fixture,
                    gzip_calls: &gzip_calls,
                },
                Admission(0),
                &mut Clock(NOW),
            );
            assert!(reopened.initialized(), "{case}");
            assert_eq!(reopened.status().channel_assertions, 1);
            assert_eq!(reopened.status().finished_successes, 0);
            let snapshot = reopened.freeze_snapshot(&mut Clock(NOW)).unwrap().unwrap();
            assert_eq!(snapshot.entries().len(), 1);
            assert_eq!(
                snapshot.frozen_at().as_bytes(),
                fixture.fields["snapshot_at"]
            );
            assert_eq!(
                snapshot.watermark_sha256().as_bytes().as_slice(),
                super::from_hex(&fixture.fields["watermark"])
            );
        }
    }
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
#[path = "support/forecast_scheduler.rs"]
mod scheduler;

/// preserve canonical bytes, gzip, stable address, receipt, and legacy cache reuse.
#[test]
fn immutable_forecast_material_matches_real_node_legacy_and_extended_receipts() {
    for (fixture_name, object_version, window, capture, expected_gzip_calls) in [
        (
            "legacy",
            ForecastEvidenceObjectVersion::Legacy,
            ForecastEvidenceWindow::OneDay,
            ForecastCaptureVersion::Legacy,
            1,
        ),
        (
            "clock",
            ForecastEvidenceObjectVersion::Legacy,
            ForecastEvidenceWindow::OneDay,
            ForecastCaptureVersion::ChannelBound(ForecastCaptureChannel::PublicGet),
            1,
        ),
        (
            "extended",
            ForecastEvidenceObjectVersion::Extended,
            ForecastEvidenceWindow::TenDays,
            ForecastCaptureVersion::ChannelBound(ForecastCaptureChannel::SchedulerRequest),
            2,
        ),
    ] {
        let fixture = Fixture::load(fixture_name);
        let gzip_calls = Cell::new(0);
        let codec = Codec {
            fixture: &fixture,
            gzip_calls: &gzip_calls,
        };
        let mut material = ForecastEvidenceMaterial::new(codec);
        let prepared = material
            .prepare(
                ForecastCaptureSource::Encoded(&fixture.fields["input"]),
                Some(window),
                capture,
            )
            .unwrap();
        assert_eq!(prepared.version(), object_version);
        assert_eq!(prepared.window(), window);
        assert_eq!(prepared.compressed_object(), fixture.fields["gzip"]);
        assert_eq!(
            prepared.object_sha256().as_slice(),
            from_hex(&fixture.fields["object"])
        );
        assert_eq!(
            prepared.edge_receipt_identity_sha256().as_slice(),
            from_hex(&fixture.fields["identity"])
        );
        assert_eq!(
            Codec {
                fixture: &fixture,
                gzip_calls: &gzip_calls
            }
            .canonical_json(prepared.object())
            .unwrap(),
            fixture.fields["canonical"],
        );
        assert_eq!(
            material.create_receipt(&prepared, NOW).unwrap(),
            fixture.fields["receipt"]
        );
        let retry = material
            .prepare(
                ForecastCaptureSource::Projected(&fixture.forecast),
                Some(window),
                capture,
            )
            .unwrap();
        assert_eq!(retry, prepared);
        assert_eq!(gzip_calls.get(), expected_gzip_calls);
        #[cfg(all(
            target_os = "linux",
            any(target_arch = "x86_64", target_arch = "aarch64")
        ))]
        native::verify(&material, &prepared, &fixture);
        // preserve the exact legacy response-row ceiling when the additive runtime is not active
        if object_version == ForecastEvidenceObjectVersion::Extended {
            assert_eq!(
                material.prepare(
                    ForecastCaptureSource::Projected(&fixture.forecast),
                    Some(window),
                    ForecastCaptureVersion::Legacy
                ),
                Err(ForecastEvidenceMaterialError::InvalidForecast),
            );
        }
    }
}
