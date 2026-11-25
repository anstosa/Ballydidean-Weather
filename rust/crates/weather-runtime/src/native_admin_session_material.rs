//! Native entropy, digest, password, and real-clock owners for admin sessions.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use scrypt::{Params, scrypt};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use weather_edge::{
    AdmittedAdminCredential, AdmittedAdminCredentialDigest, AdmittedAdminPasswordVerifier,
    AdmittedAdminPersistentAuthState, AdmittedAdminSessionDigest, AdmittedAdminSessionNow,
    AdmittedAdminSessionToken, WeatherAdminPortError, WeatherAdminSessionClock,
    WeatherAdminSessionMaterialPort,
};

const ADMIN_PASSWORD_SALT_BYTES: usize = 16;
const ADMIN_PASSWORD_HASH_BYTES: usize = 64;
const ADMIN_SESSION_TOKEN_BYTES: usize = 32;
const ADMIN_SCRYPT_LOG_N: u8 = 14;
const ADMIN_SCRYPT_R: u32 = 8;
const ADMIN_SCRYPT_P: u32 = 1;
const ADMIN_CREDENTIAL_DIGEST_DOMAIN: &[u8] = b"weather-admin-credential/v1\0";

/// Zero-state owner for native admin session entropy and token digests.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeWeatherAdminSessionMaterial;

impl NativeWeatherAdminSessionMaterial {
    /// Construct the process-independent native material owner.
    pub const fn new() -> Self {
        Self
    }
}

impl WeatherAdminSessionMaterialPort for NativeWeatherAdminSessionMaterial {
    /// Issue 32 random bytes with Node's unpadded base64url spelling.
    fn issue_token(&mut self) -> Result<AdmittedAdminSessionToken, WeatherAdminPortError> {
        let mut bytes = [0_u8; ADMIN_SESSION_TOKEN_BYTES];
        getrandom::fill(&mut bytes).map_err(|_| WeatherAdminPortError)?;
        Ok(AdmittedAdminSessionToken::admitted(
            URL_SAFE_NO_PAD.encode(bytes),
        ))
    }

    /// Hash the exact token text and retain Node's unpadded base64url digest.
    fn digest_token(
        &mut self,
        token: &AdmittedAdminSessionToken,
    ) -> Result<AdmittedAdminSessionDigest, WeatherAdminPortError> {
        let digest = Sha256::digest(token.expose().as_bytes());
        Ok(AdmittedAdminSessionDigest::admitted(
            URL_SAFE_NO_PAD.encode(digest),
        ))
    }
}

/// Zero-state real wall-clock owner for absolute admin session lifetimes.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeWeatherAdminSessionClock;

impl NativeWeatherAdminSessionClock {
    /// Construct the real process wall-clock owner.
    pub const fn new() -> Self {
        Self
    }
}

impl WeatherAdminSessionClock for NativeWeatherAdminSessionClock {
    /// Read one current Unix epoch millisecond without a reference-time fallback.
    fn now(&mut self) -> Result<AdmittedAdminSessionNow, WeatherAdminPortError> {
        let epoch_milliseconds =
            system_time_epoch_milliseconds(SystemTime::now()).ok_or(WeatherAdminPortError)?;
        Ok(AdmittedAdminSessionNow { epoch_milliseconds })
    }
}

/// Derive one 64-byte verifier with a new 16-byte native salt.
pub fn derive_native_admin_password_verifier(
    password: &str,
) -> Result<AdmittedAdminPasswordVerifier, WeatherAdminPortError> {
    // preserve the original bootstrap password admission before deriving
    if !valid_password(password, 8) {
        return Err(WeatherAdminPortError);
    }
    let mut salt = [0_u8; ADMIN_PASSWORD_SALT_BYTES];
    getrandom::fill(&mut salt).map_err(|_| WeatherAdminPortError)?;
    let hash = derive_password_hash(password, &salt)?;
    Ok(AdmittedAdminPasswordVerifier::admitted(
        hash.to_vec(),
        salt.to_vec(),
    ))
}

/// Verify one password against the original raw salt and 64-byte scrypt output.
pub fn verify_native_admin_password(
    password: &str,
    verifier: &AdmittedAdminPasswordVerifier,
) -> Result<bool, WeatherAdminPortError> {
    // preserve login password length and control-character admission
    if !valid_password(password, 1) {
        return Ok(false);
    }
    // preserve the persisted v1 material lengths before attempting derivation
    if verifier.hash().len() != ADMIN_PASSWORD_HASH_BYTES
        || verifier.salt().len() != ADMIN_PASSWORD_SALT_BYTES
    {
        return Ok(false);
    }
    let actual = derive_password_hash(password, verifier.salt())?;
    Ok(bool::from(actual.as_slice().ct_eq(verifier.hash())))
}

/// Bind one original persisted verifier to its internal typed credential identity.
pub fn admit_native_admin_persistent_auth_state(
    verifier: AdmittedAdminPasswordVerifier,
) -> Result<AdmittedAdminPersistentAuthState, WeatherAdminPortError> {
    let credential_digest = native_admin_credential_digest(&verifier)?;
    Ok(AdmittedAdminPersistentAuthState::admitted(
        credential_digest,
        verifier,
    ))
}

/// Verify one submitted password and return only its verifier-bound typed proof.
pub fn verify_native_admin_credential(
    password: &str,
    verifier: &AdmittedAdminPasswordVerifier,
) -> Result<Option<AdmittedAdminCredential>, WeatherAdminPortError> {
    // return no proof when derivation completes but the verifier does not match
    if !verify_native_admin_password(password, verifier)? {
        return Ok(None);
    }
    let credential_digest = native_admin_credential_digest(verifier)?;
    Ok(Some(AdmittedAdminCredential::verified(credential_digest)))
}

/// Reconstruct the internal identity from fixed 16-byte salt and 64-byte verifier material.
pub fn native_admin_credential_digest(
    verifier: &AdmittedAdminPasswordVerifier,
) -> Result<AdmittedAdminCredentialDigest, WeatherAdminPortError> {
    // refuse material outside the only original persisted format
    if verifier.salt().len() != ADMIN_PASSWORD_SALT_BYTES
        || verifier.hash().len() != ADMIN_PASSWORD_HASH_BYTES
    {
        return Err(WeatherAdminPortError);
    }
    let mut digest = Sha256::new();
    digest.update(ADMIN_CREDENTIAL_DIGEST_DOMAIN);
    digest.update(verifier.salt());
    digest.update(verifier.hash());
    Ok(AdmittedAdminCredentialDigest::admitted(
        URL_SAFE_NO_PAD.encode(digest.finalize()),
    ))
}

/// Preserve the predecessor's UTF-16 length and control-character admission.
fn valid_password(password: &str, minimum_length: usize) -> bool {
    let length = password.encode_utf16().count();
    (minimum_length..=256).contains(&length)
        && !password
            .chars()
            .any(|character| matches!(character, '\u{0000}'..='\u{001f}' | '\u{007f}'))
}

/// Apply the source N=16384, r=8, p=1 parameters to one UTF-8 password.
fn derive_password_hash(
    password: &str,
    salt: &[u8],
) -> Result<[u8; ADMIN_PASSWORD_HASH_BYTES], WeatherAdminPortError> {
    let parameters = Params::new(ADMIN_SCRYPT_LOG_N, ADMIN_SCRYPT_R, ADMIN_SCRYPT_P)
        .map_err(|_| WeatherAdminPortError)?;
    let mut hash = [0_u8; ADMIN_PASSWORD_HASH_BYTES];
    scrypt(password.as_bytes(), salt, &parameters, &mut hash).map_err(|_| WeatherAdminPortError)?;
    Ok(hash)
}

/// Convert one real system instant to signed Unix epoch milliseconds.
fn system_time_epoch_milliseconds(time: SystemTime) -> Option<i64> {
    // retain post-epoch and pre-epoch instants without saturation
    match time.duration_since(UNIX_EPOCH) {
        Ok(elapsed) => i64::try_from(elapsed.as_millis()).ok(),
        Err(error) => negative_epoch_milliseconds(error.duration()),
    }
}

/// Floor one pre-epoch duration to the containing signed millisecond.
fn negative_epoch_milliseconds(duration: Duration) -> Option<i64> {
    let mut magnitude = duration.as_millis();

    // preserve the preceding millisecond when sub-millisecond time remains
    if !duration.subsec_nanos().is_multiple_of(1_000_000) {
        magnitude = magnitude.checked_add(1)?;
    }
    i64::try_from(magnitude).ok()?.checked_neg()
}
