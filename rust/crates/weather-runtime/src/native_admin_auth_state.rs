//! Native filesystem persistence for the predecessor's version-one admin auth state.

use std::{
    fs::{self, DirBuilder, OpenOptions},
    io::Write,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

use weather_contracts::JsonValue;
use weather_edge::ArchiveWirePrimitives;
use weather_edge::{
    AdmittedAdminPasswordVerifier, AdmittedAdminPersistentAuthState, WeatherAdminAuthStatePort,
    WeatherAdminPortError,
};

use crate::{
    NativeWirePrimitives, admit_native_admin_persistent_auth_state,
    maintenance_canonical_json::{field, text as object_text},
};

const ADMIN_PASSWORD_HASH_BYTES: usize = 64;
const ADMIN_PASSWORD_SALT_BYTES: usize = 16;

/// Explicit filesystem owner for one optional persistent admin auth record.
#[derive(Debug)]
pub struct NativeWeatherAdminAuthState {
    path: PathBuf,
}

impl NativeWeatherAdminAuthState {
    /// Bind one configured state path without reading or creating it.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Borrow the configured path for process startup diagnostics.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Encode and atomically replace the exact four-field predecessor record.
    fn write_state(
        &self,
        state: &AdmittedAdminPersistentAuthState,
    ) -> Result<(), WeatherAdminPortError> {
        let verifier = state.verifier();

        // refuse material that cannot be represented by the original v1 record
        if verifier.hash().len() != ADMIN_PASSWORD_HASH_BYTES
            || verifier.salt().len() != ADMIN_PASSWORD_SALT_BYTES
        {
            return Err(WeatherAdminPortError);
        }
        let primitives = NativeWirePrimitives::new();
        let hash = ArchiveWirePrimitives::encode_standard_base64(&primitives, verifier.hash())
            .ok_or(WeatherAdminPortError)?;
        let salt = ArchiveWirePrimitives::encode_standard_base64(&primitives, verifier.salt())
            .ok_or(WeatherAdminPortError)?;
        let value = JsonValue::Object(vec![
            member("hash", json_string(&hash)),
            member("salt", json_string(&salt)),
            member("username", json_string(state.username())),
            member("version", JsonValue::Number(f64::from(state.version()))),
        ]);
        let bytes = ArchiveWirePrimitives::encode_ordered_json(&primitives, &value)
            .ok_or(WeatherAdminPortError)?;
        self.atomic_replace(&bytes)
    }

    /// Publish one mode-0600 temporary file through same-directory atomic rename.
    fn atomic_replace(&self, bytes: &[u8]) -> Result<(), WeatherAdminPortError> {
        let parent = nonempty_parent(&self.path);
        let mut directories = DirBuilder::new();
        directories.recursive(true).mode(0o700);
        directories
            .create(parent)
            .map_err(|_| WeatherAdminPortError)?;

        let temporary = temporary_path(&self.path)?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)
            .map_err(|_| WeatherAdminPortError)?;
        let result = (|| {
            file.write_all(bytes)?;
            drop(file);
            fs::rename(&temporary, &self.path)
        })();

        // remove only this attempt's temporary inode after a failed publish
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result.map_err(|_| WeatherAdminPortError)
    }
}

impl WeatherAdminAuthStatePort for NativeWeatherAdminAuthState {
    /// Read one optional v1 record while preserving first-run absence.
    fn read_auth_state(
        &mut self,
    ) -> Result<Option<AdmittedAdminPersistentAuthState>, WeatherAdminPortError> {
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(WeatherAdminPortError),
        };
        decode_auth_state(&bytes).ok_or(WeatherAdminPortError)
    }

    /// Replace the configured record atomically with exact ordered JSON and one LF.
    fn write_auth_state(
        &mut self,
        state: &AdmittedAdminPersistentAuthState,
    ) -> Result<(), WeatherAdminPortError> {
        self.write_state(state)
    }
}

/// Decode one original v1 record into verifier-bound typed state.
fn decode_auth_state(bytes: &[u8]) -> Option<Option<AdmittedAdminPersistentAuthState>> {
    let primitives = NativeWirePrimitives::new();
    let value = ArchiveWirePrimitives::decode_json(&primitives, bytes)?;

    // preserve the predecessor's literal null first-run representation
    if matches!(&value, JsonValue::Null) {
        return Some(None);
    }
    let JsonValue::Object(_) = &value else {
        return None;
    };
    let hash_text = object_text(&value, "hash")?;
    let salt_text = object_text(&value, "salt")?;

    // retain only the predecessor's fixed operator and contract version
    if object_text(&value, "username")? != "admin" || object_number(&value, "version")? != 1.0 {
        return None;
    }
    let hash = canonical_standard_base64(&primitives, &hash_text)?;
    let salt = canonical_standard_base64(&primitives, &salt_text)?;

    // reject truncated or extended persisted cryptographic material
    if hash.len() != ADMIN_PASSWORD_HASH_BYTES || salt.len() != ADMIN_PASSWORD_SALT_BYTES {
        return None;
    }
    let state = admit_native_admin_persistent_auth_state(AdmittedAdminPasswordVerifier::admitted(
        hash, salt,
    ))
    .ok()?;
    Some(Some(state))
}

/// Decode only the original padded standard base64 spelling.
fn canonical_standard_base64(primitives: &NativeWirePrimitives, text: &str) -> Option<Vec<u8>> {
    let decoded = ArchiveWirePrimitives::decode_standard_base64(primitives, text)?;
    (ArchiveWirePrimitives::encode_standard_base64(primitives, &decoded)?.as_str() == text)
        .then_some(decoded)
}

/// Decode one required finite numeric member.
fn object_number(value: &JsonValue, key: &str) -> Option<f64> {
    let JsonValue::Number(number) = field(value, key)? else {
        return None;
    };
    number.is_finite().then_some(*number)
}

/// Construct one ordered object member.
fn member(key: &str, value: JsonValue) -> (Vec<u16>, JsonValue) {
    (key.encode_utf16().collect(), value)
}

/// Construct one generated JSON string.
fn json_string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Resolve an empty relative parent as the current directory.
fn nonempty_parent(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

/// Create one same-directory predecessor-shaped random temporary path.
fn temporary_path(path: &Path) -> Result<PathBuf, WeatherAdminPortError> {
    let mut random = [0_u8; 6];
    getrandom::fill(&mut random).map_err(|_| WeatherAdminPortError)?;
    let mut temporary = path.as_os_str().to_os_string();
    temporary.push(format!(".{}.{}.tmp", std::process::id(), lower_hex(random)));
    Ok(PathBuf::from(temporary))
}

/// Render the six random filename bytes with Node's lowercase hexadecimal spelling.
fn lower_hex(bytes: [u8; 6]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(12);

    // emit exactly two lowercase nibbles for every random byte
    for byte in bytes {
        text.push(char::from(HEX[usize::from(byte >> 4)]));
        text.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    text
}

#[cfg(test)]
mod tests {
    use std::fs;

    use weather_edge::{
        AdmittedAdminSessionNow, WeatherAdminBootstrapStatus, WeatherAdminSessionClock,
        WeatherAdminStore,
    };

    use crate::{
        NativeWeatherAdminSessionMaterial, derive_native_admin_password_verifier,
        verify_native_admin_credential,
    };

    use super::*;

    struct FixtureDirectory(PathBuf);

    impl FixtureDirectory {
        /// Allocate one bounded unused temporary fixture path.
        fn new() -> Self {
            let mut random = [0_u8; 6];
            getrandom::fill(&mut random).expect("fixture entropy");
            Self(std::env::temp_dir().join(format!(
                "weather-native-admin-auth-{}-{}",
                std::process::id(),
                lower_hex(random)
            )))
        }
    }

    impl Drop for FixtureDirectory {
        /// Remove the bounded disposable fixture even after an assertion panic.
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    struct FixtureClock;

    impl WeatherAdminSessionClock for FixtureClock {
        /// Return one admitted ordinary session issue instant.
        fn now(&mut self) -> Result<AdmittedAdminSessionNow, WeatherAdminPortError> {
            Ok(AdmittedAdminSessionNow {
                epoch_milliseconds: 1_791_421_323_004,
            })
        }
    }

    /// Persist, reload, verify, and use one ordinary bootstrap credential.
    #[test]
    fn ordinary_auth_state_supports_one_login() {
        let fixture = FixtureDirectory::new();
        let path = fixture.0.join("private/admin/auth.json");
        let verifier = derive_native_admin_password_verifier("ordinary admin password")
            .expect("derive verifier");
        let state = admit_native_admin_persistent_auth_state(verifier.clone())
            .expect("admit persistent state");
        let mut auth = NativeWeatherAdminAuthState::new(&path);
        let mut store = WeatherAdminStore::new();
        let bootstrap = store.bootstrap(&mut auth, state).expect("bootstrap state");
        assert_eq!(bootstrap.status, WeatherAdminBootstrapStatus::Configured);

        let loaded = auth
            .read_auth_state()
            .expect("read state")
            .expect("state exists");
        let credential =
            verify_native_admin_credential("ordinary admin password", loaded.verifier())
                .expect("verify credential")
                .expect("credential matches");
        let mut clock = FixtureClock;
        let mut material = NativeWeatherAdminSessionMaterial::new();
        let response = store
            .start_session(&mut auth, &mut clock, &mut material, &credential)
            .expect("start session")
            .expect("authenticated session");
        assert_eq!(response.token.expose().len(), 43);
        assert!(fs::read(path).expect("persisted bytes").ends_with(b"\n"));
    }
}
