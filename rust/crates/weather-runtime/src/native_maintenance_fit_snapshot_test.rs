//! Source differential fixtures for native fitter snapshot assembly and inspection.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use weather_contracts::JsonValue;

use super::{
    NATIVE_FIT_EXECUTABLE, NATIVE_WIND_EXECUTABLE, NativeMaintenanceFitFamily,
    NativeMaintenanceFitSnapshotError, NativeMaintenanceFitSnapshotOwner, fit_code_files,
    reject_private_material, sha256,
};
use crate::{
    NativeMaintenanceCaptureFitSourceIdentity, NativeMaintenanceCaptureReleaseGeneration,
    NativeWirePrimitives, VerifiedNativeMaintenanceInstalledRelease,
};

/// Preserve the original predecessor temperature and rain allowlists exactly.
#[test]
fn predecessor_numerical_allowlists_match_source() {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let script = r#"
import { ADJUSTMENT_FIT_PUBLIC_CODE_ALLOWLIST } from "./scripts/research/adjustment_fit_inputs.mjs";
process.stdout.write(JSON.stringify({
  rain: ADJUSTMENT_FIT_PUBLIC_CODE_ALLOWLIST.rain,
  temperature: ADJUSTMENT_FIT_PUBLIC_CODE_ALLOWLIST.temperature,
}));
"#;
    let output = Command::new("node")
        .args(["--input-type=module", "--eval", script])
        .current_dir(repository)
        .output()
        .expect("read source allowlists");
    assert!(output.status.success());
    let source: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("decode source allowlists");
    // compare both inherited numerical families in source order
    for (family, name) in [
        (NativeMaintenanceFitFamily::Rain, "rain"),
        (NativeMaintenanceFitFamily::Temperature, "temperature"),
    ] {
        let actual = fit_code_files(
            family,
            NativeMaintenanceCaptureReleaseGeneration::NodePredecessor,
        );
        let expected = source[name].as_array().expect("source family allowlist");
        assert_eq!(actual.len(), expected.len());
        // preserve every source-to-destination binding without reordering
        for (actual, expected) in actual.iter().zip(expected) {
            assert_eq!(actual.source, expected["source"].as_str().unwrap());
            assert_eq!(
                actual.destination,
                expected["destination"].as_str().unwrap()
            );
        }
    }
}

/// Match assembled native temperature code/input identities with the source inspector.
#[test]
fn native_temperature_snapshots_match_source_inspection_and_cleanup() {
    let fixture = FixtureRelease::temperature_native();
    let owner = fixture.owner();
    let input = object(vec![
        ("zeta", JsonValue::Number(2.0)),
        (
            "alpha",
            JsonValue::Array(vec![string("ordinary"), JsonValue::Boolean(true)]),
        ),
    ]);
    let mut codec = NativeWirePrimitives::new();
    let snapshots = owner
        .create_snapshots(NativeMaintenanceFitFamily::Temperature, &input, &mut codec)
        .expect("create native temperature snapshots");
    let code_root = snapshots.code_root().to_owned();
    let input_root = snapshots.input_root().to_owned();
    let source_code = source_inspection(&code_root, "code");
    let source_input = source_inspection(&input_root, "input");

    assert_source_identity(snapshots.code_snapshot(), &source_code);
    assert_source_identity(snapshots.input_snapshot(), &source_input);
    assert_eq!(
        fs::read(input_root.join("temperature.json")).unwrap(),
        br#"{"alpha":["ordinary",true],"zeta":2}
"#
    );
    assert_eq!(
        snapshots.code_proof().generation,
        NativeMaintenanceCaptureReleaseGeneration::Native
    );
    assert_eq!(
        snapshots.code_proof().fit_executable_sha256,
        fixture.entries.get(NATIVE_FIT_EXECUTABLE).cloned()
    );
    assert_eq!(snapshots.code_proof().wind_executable_sha256, None);
    assert_eq!(snapshots.code_proof().members.len(), 1);
    let member = &snapshots.code_proof().members[0];
    assert_eq!(member.source_path, NATIVE_FIT_EXECUTABLE);
    assert_eq!(member.source_sha256, member.destination_sha256);
    snapshots.close().expect("close both issued roots");
    assert!(!code_root.exists());
    assert!(!input_root.exists());
}

/// Retain a distinct native wind generation and actual executable proof.
#[test]
fn native_wind_snapshot_contains_only_manifest_bound_executable() {
    let fixture = FixtureRelease::wind_native();
    let owner = fixture.owner();
    let mut codec = NativeWirePrimitives::new();
    let snapshots = owner
        .create_snapshots(
            NativeMaintenanceFitFamily::Wind,
            &object(vec![("rows", JsonValue::Array(Vec::new()))]),
            &mut codec,
        )
        .expect("create native wind snapshots");

    assert_eq!(snapshots.code_snapshot().file_count, 1);
    assert_eq!(snapshots.code_proof().members.len(), 1);
    assert_eq!(
        snapshots.code_proof().wind_executable_sha256,
        fixture.entries.get(NATIVE_WIND_EXECUTABLE).cloned()
    );
    assert_eq!(snapshots.code_proof().fit_executable_sha256, None);
    assert_eq!(
        fs::metadata(snapshots.code_root().join(NATIVE_WIND_EXECUTABLE))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o500
    );
    snapshots.close().expect("close native wind snapshots");
}

/// Reject source-private material before any filesystem snapshot allocation.
#[test]
fn private_input_material_is_refused() {
    let input = object(vec![("refreshToken", string("ordinary"))]);

    assert_eq!(
        reject_private_material(&input),
        Err(NativeMaintenanceFitSnapshotError::PrivateMaterial)
    );
}

/// One small immutable release fixture on native tmpfs.
struct FixtureRelease {
    entries: BTreeMap<String, String>,
    generation: NativeMaintenanceCaptureReleaseGeneration,
    parent: PathBuf,
    release_root: PathBuf,
    source_members: Vec<NativeMaintenanceCaptureFitSourceIdentity>,
}

impl FixtureRelease {
    /// Build the single native temperature executable fixture.
    fn temperature_native() -> Self {
        Self::build(
            NativeMaintenanceCaptureReleaseGeneration::Native,
            fit_code_files(
                NativeMaintenanceFitFamily::Temperature,
                NativeMaintenanceCaptureReleaseGeneration::Native,
            ),
        )
    }

    /// Build the sole native wind executable fixture.
    fn wind_native() -> Self {
        Self::build(
            NativeMaintenanceCaptureReleaseGeneration::Native,
            fit_code_files(
                NativeMaintenanceFitFamily::Wind,
                NativeMaintenanceCaptureReleaseGeneration::Native,
            ),
        )
    }

    /// Write one immutable manifest-bound release population.
    fn build(
        generation: NativeMaintenanceCaptureReleaseGeneration,
        members: Vec<super::SourceCopy>,
    ) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("fixture clock")
            .as_nanos();
        let parent = Path::new("/dev/shm").join(format!(
            "weather-fit-snapshot-release-{}-{nonce}",
            std::process::id()
        ));
        let source_commit = "a".repeat(40);
        let release_root = parent.join(&source_commit);
        fs::create_dir_all(&release_root).expect("create release root");
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o700))
            .expect("protect fixture parent");
        let mut entries = BTreeMap::new();
        let mut source_members = Vec::new();
        // create every distinct installed source once despite destination aliases
        for member in members {
            // skip a repeated installed source while retaining its first binding
            if entries.contains_key(&member.source) {
                continue;
            }
            // use the genuine reviewed entries required by the exact native transform
            let bytes = if matches!(
                member.source.as_str(),
                "scripts/research/temperature_refresh.py" | "scripts/research/rain_refresh.py"
            ) {
                let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
                fs::read(repository.join(&member.source)).expect("read transform source fixture")
            } else {
                format!("fixture:{}\n", member.source).into_bytes()
            };
            write_fixture_member(&release_root, &member.source, &bytes, member.source_mode);
            let digest = sha256(&bytes);
            entries.insert(member.source.clone(), digest.clone());
            // retain only inherited source identities in the independent source proof
            if member.source_mode == 0o400 {
                source_members.push(NativeMaintenanceCaptureFitSourceIdentity {
                    path: member.source,
                    sha256: digest,
                });
            }
        }
        let manifest = entries
            .iter()
            .map(|(path, digest)| format!("{digest}  {path}\n"))
            .collect::<String>()
            .into_bytes();
        write_fixture_member(&release_root, "MANIFEST.sha256", &manifest, 0o400);
        protect_directories(&release_root);
        Self {
            entries,
            generation,
            parent,
            release_root,
            source_members,
        }
    }

    /// Construct the snapshot owner from this independently described fixture release.
    fn owner(&self) -> NativeMaintenanceFitSnapshotOwner {
        let manifest = fs::read(self.release_root.join("MANIFEST.sha256")).unwrap();
        NativeMaintenanceFitSnapshotOwner::for_fixture(
            self.release_root.clone(),
            VerifiedNativeMaintenanceInstalledRelease {
                generation: self.generation,
                manifest_sha256: sha256(&manifest),
                member_count: self.entries.len(),
                source_commit: "a".repeat(40),
            },
            self.source_members.clone(),
        )
        .expect("bind fixture release")
    }
}

impl Drop for FixtureRelease {
    /// Restore private write modes and remove only this disposable fixture root.
    fn drop(&mut self) {
        make_writable(&self.parent);
        let _ = fs::remove_dir_all(&self.parent);
    }
}

/// Write one installed fixture member and close its final immutable mode.
fn write_fixture_member(root: &Path, relative: &str, bytes: &[u8], mode: u32) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).expect("create fixture parents");
    fs::write(&path, bytes).expect("write fixture member");
    fs::set_permissions(&path, fs::Permissions::from_mode(mode)).expect("protect fixture member");
}

/// Close every release directory to the installed immutable mode.
fn protect_directories(root: &Path) {
    let mut pending = vec![root.to_owned()];
    let mut directories = Vec::new();
    // enumerate fixture directories before their modes become immutable
    while let Some(directory) = pending.pop() {
        directories.push(directory.clone());
        // enumerate each literal child before closing directory permissions
        for entry in fs::read_dir(&directory).unwrap() {
            let path = entry.unwrap().path();
            // descend only through ordinary fixture directories
            if path.is_dir() {
                pending.push(path);
            }
        }
    }
    // close children and root after all fixture writes complete
    for directory in directories {
        fs::set_permissions(directory, fs::Permissions::from_mode(0o500)).unwrap();
    }
}

/// Restore owner write permission recursively for disposable cleanup.
fn make_writable(root: &Path) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    fs::set_permissions(root, fs::Permissions::from_mode(0o700)).ok();
    // reopen every fixture directory without changing file contents
    for entry in entries.flatten() {
        let path = entry.path();
        // recurse only through fixture directories
        if path.is_dir() {
            make_writable(&path);
        }
    }
}

/// Inspect one live fixture through the actual source implementation.
fn source_inspection(root: &Path, kind: &str) -> serde_json::Value {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let script = r#"
import { inspectAdjustmentFitSnapshot } from "./scripts/research/adjustment_fit_sandbox.mjs";
const value = await inspectAdjustmentFitSnapshot(
  process.env.WEATHER_FIT_SNAPSHOT_ROOT,
  process.env.WEATHER_FIT_SNAPSHOT_KIND,
);
process.stdout.write(JSON.stringify(value));
"#;
    let output = Command::new("node")
        .args(["--input-type=module", "--eval", script])
        .current_dir(repository)
        .env("WEATHER_FIT_SNAPSHOT_ROOT", root)
        .env("WEATHER_FIT_SNAPSHOT_KIND", kind)
        .output()
        .expect("run source snapshot inspection");
    assert!(
        output.status.success(),
        "source inspection failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("decode source inspection")
}

/// Compare one native receipt with the actual source inspection object.
fn assert_source_identity(
    native: &super::NativeMaintenanceFitSnapshotIdentity,
    source: &serde_json::Value,
) {
    assert_eq!(
        native.contract_version,
        source["contractVersion"].as_str().unwrap()
    );
    assert_eq!(native.file_count, source["fileCount"].as_u64().unwrap());
    assert_eq!(native.kind.as_str(), source["kind"].as_str().unwrap());
    assert_eq!(native.root_sha256, source["rootSha256"].as_str().unwrap());
    assert_eq!(native.total_bytes, source["totalBytes"].as_u64().unwrap());
}

/// Construct one generated JSON object fixture.
fn object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(name, value)| (name.encode_utf16().collect(), value))
            .collect(),
    )
}

/// Construct one generated JSON string fixture.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}
