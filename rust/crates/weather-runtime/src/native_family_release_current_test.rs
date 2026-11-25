use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use weather_contracts::JsonValue;

use super::{
    BASELINE_COMMIT, BASELINE_RELEASE, BASELINE_SERVER_IMAGE, BASELINE_WEB_IMAGE,
    NativeFamilyReleaseCurrentError, NativeFamilyReleaseCurrentHostFiles,
    NativeFamilyReleaseCurrentPaths, NativeFamilyReleaseLiveRuntime,
    read_native_family_release_current,
};
use crate::{FamilyCatalogImageVerification, MaintenanceAnchorStatusError, NativeWirePrimitives};

static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Disposable current-release host fixture.
struct FixtureRoot(PathBuf);

impl FixtureRoot {
    /// Create one process-unique canonical temporary root.
    fn create() -> Self {
        let sequence = FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "weather-family-release-current-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap();
        Self(fs::canonicalize(path).unwrap())
    }
}

impl Drop for FixtureRoot {
    /// Remove only the process-unique fixture tree.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Ordinary fixture runtime that confirms the exact selected live images.
#[derive(Default)]
struct FixtureRuntime {
    observed: bool,
}

impl NativeFamilyReleaseLiveRuntime for FixtureRuntime {
    /// Admit only the baseline release and its exact server/web references.
    fn verify_running_images(
        &mut self,
        release: &str,
        environment: &JsonValue,
    ) -> Result<(), NativeFamilyReleaseCurrentError> {
        self.observed = release == BASELINE_RELEASE
            && fixture_text(environment, "WEATHER_SERVER_IMAGE") == BASELINE_SERVER_IMAGE
            && fixture_text(environment, "WEATHER_WEB_IMAGE") == BASELINE_WEB_IMAGE;
        self.observed
            .then_some(())
            .ok_or(NativeFamilyReleaseCurrentError::Runtime)
    }
}

/// Ordinary fixture compiled-catalog owner recording the exact image and bytes.
#[derive(Default)]
struct FixtureVerifier {
    observed: bool,
}

impl FamilyCatalogImageVerification for FixtureVerifier {
    /// Admit only the empty canonical v3 catalog under the baseline server image.
    fn verify_catalog(
        &mut self,
        image: &str,
        bytes: &[u8],
    ) -> Result<(), MaintenanceAnchorStatusError> {
        self.observed = image == BASELINE_SERVER_IMAGE
            && bytes
                == b"{\"contractVersion\":\"adjustment-installed-candidate-catalog/v3\",\"entries\":[]}\n";
        self.observed
            .then_some(())
            .ok_or(MaintenanceAnchorStatusError::Slot)
    }
}

/// Read the exact baseline projection from genuine no-follow disposable host files.
#[test]
fn reads_baseline_family_current_from_fixed_host_material() {
    let root = FixtureRoot::create();
    let owner = fs::metadata(&root.0).unwrap();
    assert_eq!(
        owner.uid(),
        owner.gid(),
        "host-file source requires uid=gid"
    );
    let releases = root.0.join("releases");
    let state = root.0.join("state");
    fs::create_dir(&releases).unwrap();
    fs::create_dir(&state).unwrap();
    let current_release = root.0.join("current-release");
    let settings = root.0.join("settings.json");
    let catalog = root.0.join("catalog.json");
    write_file(
        &current_release,
        format!("{BASELINE_RELEASE}\n").as_bytes(),
        0o600,
    );
    write_file(
        &releases.join(format!("{BASELINE_RELEASE}.env")),
        fixture_environment().as_bytes(),
        0o600,
    );
    write_file(
        &settings,
        b"{\"rain\":true,\"temperature\":true,\"version\":1,\"wind\":false}\n",
        0o600,
    );
    let catalog_bytes =
        b"{\"contractVersion\":\"adjustment-installed-candidate-catalog/v3\",\"entries\":[]}\n";
    write_file(&catalog, catalog_bytes, 0o644);

    let paths = NativeFamilyReleaseCurrentPaths {
        current_release,
        releases,
        state_root: state,
        settings,
        catalog,
    };
    let mut files = NativeFamilyReleaseCurrentHostFiles::for_paths(
        paths,
        owner.uid(),
        [owner.uid(), owner.uid()],
        owner.uid(),
        owner.gid(),
    );
    let mut runtime = FixtureRuntime::default();
    let mut verifier = FixtureVerifier::default();
    let mut codec = NativeWirePrimitives;
    let current = read_native_family_release_current(
        "temperature",
        &mut files,
        &mut runtime,
        &mut verifier,
        &mut codec,
        &NativeWirePrimitives,
    )
    .unwrap();

    assert!(runtime.observed);
    assert!(verifier.observed);
    assert_eq!(current.commit, BASELINE_COMMIT);
    assert_eq!(current.release, BASELINE_RELEASE);
    assert_eq!(current.family, "temperature");
    assert_eq!(current.active_installed_receipt_sha256, None);
    assert_eq!(current.shadow_installed_receipt_sha256, None);
    assert_eq!(current.control_installed_receipt_sha256, None);
    assert_eq!(
        current.source_server_image_digest,
        BASELINE_SERVER_IMAGE.split('@').nth(1).unwrap()
    );
}

/// Write one fixture file with its exact final mode.
fn write_file(path: &Path, bytes: &[u8], mode: u32) {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(path)
        .unwrap();
    file.write_all(bytes).unwrap();
    file.sync_all().unwrap();
    drop(file);
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}

/// Build the exact closed eleven-line baseline environment.
fn fixture_environment() -> String {
    format!(
        "CLOUDFLARED_IMAGE=cloudflare/cloudflared@sha256:{}\n\
         POSTGRES_IMAGE=postgres@sha256:{}\n\
         WEATHER_CONTROL_PLANE_SHA256={}\n\
         WEATHER_CONTROL_PLANE_VERSION=14\n\
         WEATHER_DATABASE_NAME=weather\n\
         WEATHER_FORECAST_ADJUSTMENT_TEMPERATURE_CANARY_KILL_SWITCH=0\n\
         WEATHER_FORECAST_ADJUSTMENT_WIND_CANARY_KILL_SWITCH=0\n\
         WEATHER_POSTGRES_DIR=/var/lib/postgresql/data\n\
         WEATHER_RELEASE={BASELINE_RELEASE}\n\
         WEATHER_SERVER_IMAGE={BASELINE_SERVER_IMAGE}\n\
         WEATHER_WEB_IMAGE={BASELINE_WEB_IMAGE}\n",
        "1".repeat(64),
        "2".repeat(64),
        "3".repeat(64),
    )
}

/// Read one exact fixture object string member.
fn fixture_text(value: &JsonValue, key: &str) -> String {
    let JsonValue::Object(fields) = value else {
        panic!("fixture environment object");
    };
    let JsonValue::String(units) = fields
        .iter()
        .find_map(|(name, value)| name.iter().copied().eq(key.encode_utf16()).then_some(value))
        .unwrap()
    else {
        panic!("fixture environment string");
    };
    String::from_utf16(units).unwrap()
}
