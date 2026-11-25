//! Compiled-Node parity for the shared original Open-Meteo backfill arguments.

use std::{path::PathBuf, process::Command};

use weather_contracts::Nullable;
use weather_worker::{
    HistoricalBackfillCommandDate, HistoricalBackfillCommandTimeAdapter,
    OpenMeteoBackfillArgumentError, TaskFailure, parse_open_meteo_backfill_arguments,
};

const TODAY_MS: i64 = 1_787_356_800_000;

struct Fixture {
    name: &'static str,
    arguments: &'static [&'static str],
}

struct FixtureTime;

impl HistoricalBackfillCommandTimeAdapter for FixtureTime {
    /// return the exact UTC date supplied to the compiled Node parser
    fn today_utc(&mut self) -> Result<HistoricalBackfillCommandDate, TaskFailure> {
        Ok(HistoricalBackfillCommandDate::admitted(
            "2026-08-22",
            TODAY_MS,
        ))
    }

    /// admit only the explicit dates exercised by this differential corpus
    fn admit_utc_date(
        &mut self,
        label: &str,
    ) -> Result<HistoricalBackfillCommandDate, TaskFailure> {
        let start_ms = match label {
            "2024-02-29" => 1_709_164_800_000,
            "2024-03-01" => 1_709_251_200_000,
            "2026-08-01" => 1_785_542_400_000,
            "2026-08-20" => 1_787_184_000_000,
            "2026-08-21" => 1_787_270_400_000,
            "2026-08-22" => TODAY_MS,
            // preserve the original rollover refusal without implementing a calendar codec
            "2025-02-29" => return Err(failure()),
            _ => return Err(failure()),
        };
        Ok(HistoricalBackfillCommandDate::admitted(label, start_ms))
    }

    /// keep unrelated calendar arithmetic outside argument admission
    fn add_utc_days(
        &mut self,
        _: &HistoricalBackfillCommandDate,
        _: i64,
    ) -> Result<HistoricalBackfillCommandDate, TaskFailure> {
        unreachable!("argument parsing must not add calendar days")
    }
}

/// compare every Rust result and literal diagnostic to one compiled Node child
#[test]
fn matches_compiled_open_meteo_backfill_argument_contract() {
    let fixtures = fixtures();
    let oracle = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/open_meteo_backfill_arguments_node_oracle.mjs");
    let mut command = Command::new("node");
    command.arg(oracle);
    // send one self-delimiting argv corpus without introducing a document codec
    for fixture in &fixtures {
        command
            .arg(fixture.name)
            .arg(fixture.arguments.len().to_string())
            .args(fixture.arguments);
    }
    let output = command
        .output()
        .expect("compiled Node parser oracle starts");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("oracle output is UTF-8");
    let mut lines = stdout.lines();

    // compare cases in the exact generated argv order
    for fixture in &fixtures {
        let line = lines
            .next()
            .unwrap_or_else(|| panic!("missing Node outcome for {}", fixture.name));
        compare_fixture(fixture, line);
    }
    assert!(lines.next().is_none(), "unexpected extra Node outcome");
}

/// compare one typed Rust outcome with the oracle's fixed tab projection
fn compare_fixture(fixture: &Fixture, line: &str) {
    let fields = line.split('\t').collect::<Vec<_>>();
    assert_eq!(fields[0], fixture.name, "oracle case order");
    let arguments = fixture
        .arguments
        .iter()
        .map(|value| (*value).to_owned())
        .collect::<Vec<_>>();
    let result = parse_open_meteo_backfill_arguments(&arguments, &mut FixtureTime);
    match result {
        Ok(parsed) => {
            assert_eq!(fields.len(), 10, "success fields for {}", fixture.name);
            assert_eq!(fields[1], "ok", "Node rejected {:?}", fixture.arguments);
            let (arguments, _, _) = parsed.into_parts();
            let actual = [
                arguments.chunkDays.to_string(),
                arguments.dryRun.to_string(),
                arguments.from,
                nullable_text(&arguments.reportPath).to_owned(),
                arguments.resume.to_string(),
                arguments.site,
                nullable_text(&arguments.source).to_owned(),
                arguments.to,
            ];
            let expected = fields[2..]
                .iter()
                .map(|value| (*value).to_owned())
                .collect::<Vec<_>>();
            assert_eq!(
                actual.as_slice(),
                expected.as_slice(),
                "{} argv {:?}",
                fixture.name,
                fixture.arguments
            );
        }
        Err(error) => {
            assert_eq!(fields.len(), 4, "error fields for {}", fixture.name);
            assert_eq!(fields[1], "err", "Node accepted {:?}", fixture.arguments);
            assert_eq!(fields[2], javascript_error_name(&error), "{}", fixture.name);
            assert_eq!(
                fields[3],
                error.to_string(),
                "{} argv {:?}",
                fixture.name,
                fixture.arguments
            );
        }
    }
}

/// project one generated nullable string without serializing its contract
fn nullable_text(value: &Nullable<String>) -> &str {
    match value {
        Nullable::Null => "<null>",
        Nullable::Value(value) => value,
    }
}

/// retain the original JavaScript error class beside its literal message
const fn javascript_error_name(error: &OpenMeteoBackfillArgumentError) -> &'static str {
    match error {
        OpenMeteoBackfillArgumentError::UnsupportedArgument(_)
        | OpenMeteoBackfillArgumentError::RequiresValue(_)
        | OpenMeteoBackfillArgumentError::RequiredArgument(_) => "Error",
        OpenMeteoBackfillArgumentError::InvalidDate(_)
        | OpenMeteoBackfillArgumentError::InvalidTimeValue
        | OpenMeteoBackfillArgumentError::ReversedRange
        | OpenMeteoBackfillArgumentError::NonPastRange
        | OpenMeteoBackfillArgumentError::InvalidChunkDays => "RangeError",
        OpenMeteoBackfillArgumentError::Clock(_) => "Error",
    }
}

/// provide one bounded failure from the explicit fixture time owner
fn failure() -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: "fixture date unavailable".to_owned(),
    }
}

/// define the ordered original argument corpus shared through the child argv
fn fixtures() -> Vec<Fixture> {
    vec![
        Fixture {
            name: "minimal-defaults",
            arguments: &[
                "--site",
                "ballydidean",
                "--from",
                "2026-08-21",
                "--to",
                "2026-08-21",
            ],
        },
        Fixture {
            name: "reordered-full-raw",
            arguments: &[
                "--resume",
                "--report",
                " report.json ",
                "--to",
                "2026-08-20",
                "--dry-run",
                "--source",
                " source-v1 ",
                "--from",
                "2026-08-01",
                "--site",
                " ballydidean ",
                "--chunk-days",
                "7",
                "--resume",
                "--dry-run",
            ],
        },
        Fixture {
            name: "last-write-wins",
            arguments: &[
                "--site",
                "first",
                "--from",
                "bad",
                "--to",
                "bad",
                "--source",
                "first-source",
                "--chunk-days",
                "0",
                "--report",
                "first-report",
                "--site",
                "final",
                "--from",
                "2026-08-01",
                "--to",
                "2026-08-20",
                "--source",
                "final-source",
                "--chunk-days",
                "14",
                "--report",
                "final-report",
            ],
        },
        Fixture {
            name: "number-radix-trim",
            arguments: &[
                "--site",
                "ballydidean",
                "--from",
                "2026-08-21",
                "--to",
                "2026-08-21",
                "--chunk-days",
                " 0x0e ",
            ],
        },
        Fixture {
            name: "missing-site",
            arguments: &["--from", "2026-08-21", "--to", "2026-08-21"],
        },
        Fixture {
            name: "bom-blank-site",
            arguments: &[
                "--site",
                "\u{feff}",
                "--from",
                "2026-08-21",
                "--to",
                "2026-08-21",
            ],
        },
        Fixture {
            name: "missing-from",
            arguments: &["--site", "ballydidean", "--to", "2026-08-21"],
        },
        Fixture {
            name: "missing-to",
            arguments: &["--site", "ballydidean", "--from", "2026-08-21"],
        },
        Fixture {
            name: "requires-value-before-option",
            arguments: &["--site", "--from", "2026-08-21", "--to", "2026-08-21"],
        },
        Fixture {
            name: "unknown-first",
            arguments: &["--unknown"],
        },
        Fixture {
            name: "valid-leap-day",
            arguments: &[
                "--site",
                "ballydidean",
                "--from",
                "2024-02-29",
                "--to",
                "2024-03-01",
            ],
        },
        Fixture {
            name: "invalid-leap-day",
            arguments: &[
                "--site",
                "ballydidean",
                "--from",
                "2025-02-29",
                "--to",
                "2025-03-01",
            ],
        },
        Fixture {
            name: "invalid-month-native-time",
            arguments: &[
                "--site",
                "ballydidean",
                "--from",
                "2025-13-01",
                "--to",
                "2026-08-21",
            ],
        },
        Fixture {
            name: "reversed-range",
            arguments: &[
                "--site",
                "ballydidean",
                "--from",
                "2026-08-21",
                "--to",
                "2026-08-20",
            ],
        },
        Fixture {
            name: "current-day-range",
            arguments: &[
                "--site",
                "ballydidean",
                "--from",
                "2026-08-21",
                "--to",
                "2026-08-22",
            ],
        },
        Fixture {
            name: "fractional-chunk-days",
            arguments: &[
                "--site",
                "ballydidean",
                "--from",
                "2026-08-21",
                "--to",
                "2026-08-21",
                "--chunk-days",
                "14.5",
            ],
        },
    ]
}
