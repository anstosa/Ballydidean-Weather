use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use weather_contracts::JsonValue;

use crate::maintenance_canonical_json::text;
use crate::{
    MaintenanceCanonicalJson, NativeFamilyReleaseCurrentError, NativeFamilyReleaseCurrentFiles,
    NativeFamilyReleaseTargetAuthorityRead, NativeInstalledFamilyReleaseTargetRead,
    NativeWirePrimitives,
};

const HASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

struct FixtureCurrentFiles {
    environment: Vec<u8>,
}

impl NativeFamilyReleaseCurrentFiles for FixtureCurrentFiles {
    /// The target reader never selects the current release marker directly.
    fn read_current_release(&mut self) -> Result<Vec<u8>, NativeFamilyReleaseCurrentError> {
        Err(NativeFamilyReleaseCurrentError::File)
    }

    /// Return the exact fixture environment only for its bound release.
    fn read_environment(
        &mut self,
        release: &str,
    ) -> Result<Vec<u8>, NativeFamilyReleaseCurrentError> {
        // preserve the production reader's release selection boundary
        if release != "2026.11.25-1" {
            return Err(NativeFamilyReleaseCurrentError::Release);
        }
        Ok(self.environment.clone())
    }

    /// Return an ordinary nonempty settings member for the delegated read.
    fn read_settings(&mut self) -> Result<Vec<u8>, NativeFamilyReleaseCurrentError> {
        Ok(b"settings".to_vec())
    }

    /// Return an ordinary nonempty catalog member for the delegated read.
    fn read_catalog(&mut self) -> Result<Vec<u8>, NativeFamilyReleaseCurrentError> {
        Ok(b"catalog".to_vec())
    }

    /// The target reader never selects the current acknowledgement directly.
    fn read_current_authority(
        &mut self,
    ) -> Result<Option<Vec<u8>>, NativeFamilyReleaseCurrentError> {
        Err(NativeFamilyReleaseCurrentError::File)
    }
}

/// Read exact installed environment and capacity bytes through the concrete target authority.
#[test]
fn native_family_release_installed_reads_exact_proof_inputs() {
    let root = temporary_root();
    fs::create_dir(&root).expect("create fixture root");
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).expect("set fixture root mode");
    let owner = fs::metadata(&root).expect("read fixture root owner");
    let capacity = JsonValue::Object(vec![
        (
            "actionSha256".encode_utf16().collect(),
            JsonValue::String(HASH.encode_utf16().collect()),
        ),
        (
            "contractVersion".encode_utf16().collect(),
            JsonValue::String(
                "adjustment-family-release-capacity/v1"
                    .encode_utf16()
                    .collect(),
            ),
        ),
    ]);
    let capacity_bytes =
        MaintenanceCanonicalJson::encode_canonical_json(&mut NativeWirePrimitives, &capacity)
            .expect("encode canonical capacity");
    let capacity_path = root.join(format!("capacity-sha256-{HASH}.json"));
    fs::write(&capacity_path, &capacity_bytes).expect("write capacity");
    fs::set_permissions(&capacity_path, fs::Permissions::from_mode(0o600))
        .expect("set capacity mode");

    let mut reader = NativeInstalledFamilyReleaseTargetRead::for_parts(
        FixtureCurrentFiles {
            environment: fixture_environment(),
        },
        root.clone(),
        owner.uid(),
        owner.gid(),
    );
    let environment = reader
        .read_release_environment("2026.11.25-1")
        .expect("read environment");
    let observed_capacity = reader.read_capacity(HASH).expect("read capacity");

    assert_eq!(
        text(&environment, "WEATHER_SERVER_IMAGE").as_deref(),
        Some(
            "ghcr.io/anstosa/weather-server@sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
        )
    );
    assert_eq!(
        text(&observed_capacity, "actionSha256").as_deref(),
        Some(HASH)
    );
    fs::remove_dir_all(root).expect("remove fixture root");
}

// return one unique bounded disposable fixture root without persistent allocation
fn temporary_root() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("fixture time")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "weather-family-release-installed-{}-{nonce}",
        std::process::id()
    ))
}

// preserve the fixed closed eleven-line installed release environment
fn fixture_environment() -> Vec<u8> {
    format!(
        concat!(
            "CLOUDFLARED_IMAGE=cloudflare/cloudflared@sha256:{hash}\n",
            "POSTGRES_IMAGE=postgres@sha256:{hash}\n",
            "WEATHER_CONTROL_PLANE_SHA256={hash}\n",
            "WEATHER_CONTROL_PLANE_VERSION=17\n",
            "WEATHER_DATABASE_NAME=weather\n",
            "WEATHER_FORECAST_ADJUSTMENT_TEMPERATURE_CANARY_KILL_SWITCH=0\n",
            "WEATHER_FORECAST_ADJUSTMENT_WIND_CANARY_KILL_SWITCH=0\n",
            "WEATHER_POSTGRES_DIR=/var/lib/postgresql/data\n",
            "WEATHER_RELEASE=2026.11.25-1\n",
            "WEATHER_SERVER_IMAGE=ghcr.io/anstosa/weather-server@sha256:{server}\n",
            "WEATHER_WEB_IMAGE=ghcr.io/anstosa/weather-web@sha256:{web}\n"
        ),
        hash = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        server = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        web = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
    )
    .into_bytes()
}
