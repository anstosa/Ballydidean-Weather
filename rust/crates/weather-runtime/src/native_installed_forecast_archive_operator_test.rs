//! Source differential for the installed native forecast archive idle response.

use std::{
    fs,
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

use weather_edge::{
    NativeForecastArchiveOperator, parse_forecast_archive_operator_command,
    run_forecast_archive_operator_command,
};

use crate::{
    NativeForecastAllocationStatfs, NativeForecastArchiveCodec, NativeProcessClock,
    NativeWirePrimitives,
};

static NEXT_FIXTURE_ID: AtomicU64 = AtomicU64::new(0);

struct FixtureRoot(PathBuf);

impl FixtureRoot {
    /// Create separate empty roots for the source and native storage owners.
    fn new() -> Self {
        let id = NEXT_FIXTURE_ID.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "weather-native-forecast-archive-operator-{}-{id}",
            std::process::id()
        ));
        // create owner-private roots under the ordinary CI umask
        let mut directories = fs::DirBuilder::new();
        directories
            .mode(0o700)
            .create(&root)
            .expect("create fixture root");
        directories
            .create(root.join("node"))
            .expect("create Node archive root");
        directories
            .create(root.join("native"))
            .expect("create native archive root");
        Self(root)
    }

    /// Borrow one implementation-specific empty archive root.
    fn child(&self, name: impl AsRef<Path>) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for FixtureRoot {
    /// Remove every online archive directory created by both implementations.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Refuse any accidental compression on the idle command path.
fn unexpected_compression(_: &[u8]) -> Option<Vec<u8>> {
    panic!("empty archive-next unexpectedly invoked forecast compression")
}

/// Match the source command's exact idle stdout on separate disposable empty roots.
#[test]
fn native_empty_archive_next_matches_source_idle_stdout() {
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("canonical repository root");
    let fixture = FixtureRoot::new();
    let source = source_archive_next(&repository, &fixture.child("node"));

    let codec =
        NativeForecastArchiveCodec::new(unexpected_compression as fn(&[u8]) -> Option<Vec<u8>>);
    let calendar = NativeWirePrimitives;
    let mut application = NativeForecastArchiveOperator::initialize(
        fixture.child("native"),
        &codec,
        &calendar,
        NativeProcessClock,
        NativeForecastAllocationStatfs::new(),
    )
    .expect("initialize genuine native archive operator");
    let command = parse_forecast_archive_operator_command("archive-next", &[])
        .expect("admit archive-next")
        .expect("recognized archive-next");
    let mut native = Vec::new();
    run_forecast_archive_operator_command(command, &mut application, &mut native)
        .expect("run native archive-next");

    assert_eq!(native, source);
    assert_eq!(
        native,
        br#"{"contractVersion":"adjustment-archive-transfer/v1","kind":"idle"}
"#
    );
}

/// Execute the original source entrypoint against only one disposable configured root.
fn source_archive_next(repository: &Path, root: &Path) -> Vec<u8> {
    let output = Command::new("node")
        .args([
            "--input-type=module",
            "--eval",
            r#"import { pathToFileURL } from "node:url";
const source = await import(pathToFileURL(process.env.WEATHER_TEST_ARCHIVE_MODULE).href);
await source.runAdjustmentArchiveCommand(["archive-next"], {
  root: process.env.WEATHER_ADJUSTMENT_EVIDENCE_ROOT,
});"#,
        ])
        .current_dir(repository)
        .env("WEATHER_ADJUSTMENT_EVIDENCE_ROOT", root)
        .env(
            "WEATHER_TEST_ARCHIVE_MODULE",
            repository.join("deploy/scripts/adjustment-evidence-store.mjs"),
        )
        .output()
        .expect("run source archive-next");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}
