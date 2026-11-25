use std::collections::VecDeque;
use std::fmt;

pub const ADMIN_SESSION_TTL_SECONDS: u64 = 12 * 60 * 60;
pub const MAXIMUM_ADMIN_SESSIONS: usize = 64;
const ADMIN_SESSION_TTL_MILLISECONDS: i64 = ADMIN_SESSION_TTL_SECONDS as i64 * 1_000;

/// One admitted persistent password-verifier identity without verifier material.
#[derive(Clone, PartialEq, Eq)]
pub struct AdmittedAdminCredentialDigest(String);

impl AdmittedAdminCredentialDigest {
    /// Retain one digest already produced and verified by the crypto adapter.
    pub fn admitted(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

impl fmt::Debug for AdmittedAdminCredentialDigest {
    /// Redact credential identity from diagnostic output.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AdmittedAdminCredentialDigest(<redacted>)")
    }
}

/// One externally derived password verifier and salt for persistent storage.
#[derive(Clone, PartialEq, Eq)]
pub struct AdmittedAdminPasswordVerifier {
    hash: Vec<u8>,
    salt: Vec<u8>,
}

impl AdmittedAdminPasswordVerifier {
    /// Retain verifier bytes already derived by the crypto adapter.
    pub fn admitted(hash: Vec<u8>, salt: Vec<u8>) -> Self {
        Self { hash, salt }
    }

    /// Expose verifier bytes only to the persistent auth-state adapter.
    pub fn hash(&self) -> &[u8] {
        &self.hash
    }

    /// Expose salt bytes only to the persistent auth-state adapter.
    pub fn salt(&self) -> &[u8] {
        &self.salt
    }
}

impl fmt::Debug for AdmittedAdminPasswordVerifier {
    /// Redact verifier and salt material from diagnostic output.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AdmittedAdminPasswordVerifier(<redacted>)")
    }
}

/// Fixed v1 admin auth state owned by the persistence adapter.
#[derive(Clone, PartialEq, Eq)]
pub struct AdmittedAdminPersistentAuthState {
    credential_digest: AdmittedAdminCredentialDigest,
    verifier: AdmittedAdminPasswordVerifier,
}

impl AdmittedAdminPersistentAuthState {
    /// Bind one already encoded v1 admin credential state to its admitted digest.
    pub fn admitted(
        credential_digest: AdmittedAdminCredentialDigest,
        verifier: AdmittedAdminPasswordVerifier,
    ) -> Self {
        Self {
            credential_digest,
            verifier,
        }
    }

    /// Return the fixed persistent auth contract version.
    pub const fn version(&self) -> u8 {
        1
    }

    /// Return the fixed persistent operator name.
    pub const fn username(&self) -> &'static str {
        "admin"
    }

    /// Borrow verifier material only for typed persistence.
    pub fn verifier(&self) -> &AdmittedAdminPasswordVerifier {
        &self.verifier
    }
}

impl fmt::Debug for AdmittedAdminPersistentAuthState {
    /// Redact every credential-bearing persistent field.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AdmittedAdminPersistentAuthState")
            .field("version", &self.version())
            .field("username", &self.username())
            .field("credential", &"<redacted>")
            .finish()
    }
}

/// One credential proof already verified against a persistent state digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedAdminCredential {
    credential_digest: AdmittedAdminCredentialDigest,
}

impl AdmittedAdminCredential {
    /// Retain the persistent credential identity proven by the crypto adapter.
    pub fn verified(credential_digest: AdmittedAdminCredentialDigest) -> Self {
        Self { credential_digest }
    }
}

/// One syntax-admitted opaque browser session token.
#[derive(Clone, PartialEq, Eq)]
pub struct AdmittedAdminSessionToken(String);

impl AdmittedAdminSessionToken {
    /// Retain one opaque token admitted by the random-token adapter.
    pub fn admitted(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Expose the token only to the response or digest adapter.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for AdmittedAdminSessionToken {
    /// Redact the opaque session token from diagnostic output.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AdmittedAdminSessionToken(<redacted>)")
    }
}

/// One admitted digest of an opaque browser session token.
#[derive(Clone, PartialEq, Eq)]
pub struct AdmittedAdminSessionDigest(String);

impl AdmittedAdminSessionDigest {
    /// Retain one digest already produced by the token-digest adapter.
    pub fn admitted(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

impl fmt::Debug for AdmittedAdminSessionDigest {
    /// Redact the retained token digest from diagnostic output.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AdmittedAdminSessionDigest(<redacted>)")
    }
}

/// One admitted real wall-clock sample for session lifecycle decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdmittedAdminSessionNow {
    pub epoch_milliseconds: i64,
}

/// One successful bootstrap response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeatherAdminBootstrapStatus {
    Configured,
    AlreadyConfigured,
}

/// Exact bootstrap response metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WeatherAdminBootstrapResponse {
    pub status: WeatherAdminBootstrapStatus,
}

/// Exact successful login response metadata.
#[derive(Clone, PartialEq, Eq)]
pub struct WeatherAdminSessionResponse {
    pub maximum_age_seconds: u64,
    pub token: AdmittedAdminSessionToken,
}

impl fmt::Debug for WeatherAdminSessionResponse {
    /// Redact the response token while retaining public metadata.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WeatherAdminSessionResponse")
            .field("maximum_age_seconds", &self.maximum_age_seconds)
            .field("token", &"<redacted>")
            .finish()
    }
}

/// Bounded injected port failure without credential or token detail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WeatherAdminPortError;

/// One ordinary store lifecycle failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeatherAdminStoreError {
    Clock,
    ExpirationOverflow,
    Persistence,
    SessionMaterial,
}

/// Typed persistent auth state ownership boundary.
pub trait WeatherAdminAuthStatePort {
    /// Read the one optional persistent v1 auth state.
    fn read_auth_state(
        &mut self,
    ) -> Result<Option<AdmittedAdminPersistentAuthState>, WeatherAdminPortError>;

    /// Persist the first admitted v1 auth state.
    fn write_auth_state(
        &mut self,
        state: &AdmittedAdminPersistentAuthState,
    ) -> Result<(), WeatherAdminPortError>;
}

/// Real wall-clock boundary for issue and absolute-expiry decisions.
pub trait WeatherAdminSessionClock {
    /// Read one admitted real wall-clock sample.
    fn now(&mut self) -> Result<AdmittedAdminSessionNow, WeatherAdminPortError>;
}

/// Random token and digest operations supplied by admitted external adapters.
pub trait WeatherAdminSessionMaterialPort {
    /// Issue one syntax-admitted opaque token.
    fn issue_token(&mut self) -> Result<AdmittedAdminSessionToken, WeatherAdminPortError>;

    /// Digest one admitted token without exposing its material to the store.
    fn digest_token(
        &mut self,
        token: &AdmittedAdminSessionToken,
    ) -> Result<AdmittedAdminSessionDigest, WeatherAdminPortError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RetainedAdminSession {
    digest: AdmittedAdminSessionDigest,
    expires_at_epoch_milliseconds: i64,
}

/// In-memory bounded session registry backed by typed persistent auth state.
#[derive(Debug, Default)]
pub struct WeatherAdminStore {
    sessions: VecDeque<RetainedAdminSession>,
}

impl WeatherAdminStore {
    /// Construct an empty process-owned session registry.
    pub const fn new() -> Self {
        Self {
            sessions: VecDeque::new(),
        }
    }

    /// Persist exactly one already verified bootstrap credential state.
    pub fn bootstrap(
        &mut self,
        auth: &mut dyn WeatherAdminAuthStatePort,
        state: AdmittedAdminPersistentAuthState,
    ) -> Result<WeatherAdminBootstrapResponse, WeatherAdminStoreError> {
        let existing = auth
            .read_auth_state()
            .map_err(|_| WeatherAdminStoreError::Persistence)?;

        // prevent replacement after the first persistent credential exists
        if existing.is_some() {
            return Ok(WeatherAdminBootstrapResponse {
                status: WeatherAdminBootstrapStatus::AlreadyConfigured,
            });
        }

        auth.write_auth_state(&state)
            .map_err(|_| WeatherAdminStoreError::Persistence)?;
        Ok(WeatherAdminBootstrapResponse {
            status: WeatherAdminBootstrapStatus::Configured,
        })
    }

    /// Exchange one already verified credential for a bounded opaque session.
    pub fn start_session(
        &mut self,
        auth: &mut dyn WeatherAdminAuthStatePort,
        clock: &mut dyn WeatherAdminSessionClock,
        material: &mut dyn WeatherAdminSessionMaterialPort,
        credential: &AdmittedAdminCredential,
    ) -> Result<Option<WeatherAdminSessionResponse>, WeatherAdminStoreError> {
        let state = auth
            .read_auth_state()
            .map_err(|_| WeatherAdminStoreError::Persistence)?;

        // keep login inactive until the admitted proof matches persisted state
        let Some(state) = state else {
            return Ok(None);
        };
        if state.credential_digest != credential.credential_digest {
            return Ok(None);
        }

        let now = clock.now().map_err(|_| WeatherAdminStoreError::Clock)?;
        let token = material
            .issue_token()
            .map_err(|_| WeatherAdminStoreError::SessionMaterial)?;
        self.prune_sessions(now.epoch_milliseconds);

        // evict exactly one oldest retained session at the source cap
        if self.sessions.len() >= MAXIMUM_ADMIN_SESSIONS {
            self.sessions.pop_front();
        }

        let digest = material
            .digest_token(&token)
            .map_err(|_| WeatherAdminStoreError::SessionMaterial)?;
        let expires_at_epoch_milliseconds = now
            .epoch_milliseconds
            .checked_add(ADMIN_SESSION_TTL_MILLISECONDS)
            .ok_or(WeatherAdminStoreError::ExpirationOverflow)?;

        // retain Map-compatible replacement without changing existing order
        if let Some(existing) = self
            .sessions
            .iter_mut()
            .find(|session| session.digest == digest)
        {
            existing.expires_at_epoch_milliseconds = expires_at_epoch_milliseconds;
        } else {
            self.sessions.push_back(RetainedAdminSession {
                digest,
                expires_at_epoch_milliseconds,
            });
        }

        Ok(Some(WeatherAdminSessionResponse {
            maximum_age_seconds: ADMIN_SESSION_TTL_SECONDS,
            token,
        }))
    }

    /// Read one active business session and expire it at its absolute deadline.
    pub fn authenticate_session(
        &mut self,
        clock: &mut dyn WeatherAdminSessionClock,
        material: &mut dyn WeatherAdminSessionMaterialPort,
        token: &AdmittedAdminSessionToken,
    ) -> Result<bool, WeatherAdminStoreError> {
        let digest = material
            .digest_token(token)
            .map_err(|_| WeatherAdminStoreError::SessionMaterial)?;
        let Some(index) = self
            .sessions
            .iter()
            .position(|session| session.digest == digest)
        else {
            return Ok(false);
        };
        let now = clock.now().map_err(|_| WeatherAdminStoreError::Clock)?;

        // remove a known session exactly at or after its absolute deadline
        if self.sessions[index].expires_at_epoch_milliseconds <= now.epoch_milliseconds {
            self.sessions.remove(index);
            return Ok(false);
        }
        Ok(true)
    }

    /// Revoke one admitted opaque session without reading the clock.
    pub fn revoke_session(
        &mut self,
        material: &mut dyn WeatherAdminSessionMaterialPort,
        token: &AdmittedAdminSessionToken,
    ) -> Result<(), WeatherAdminStoreError> {
        let digest = material
            .digest_token(token)
            .map_err(|_| WeatherAdminStoreError::SessionMaterial)?;

        // remove only the matching retained session while preserving order
        if let Some(index) = self
            .sessions
            .iter()
            .position(|session| session.digest == digest)
        {
            self.sessions.remove(index);
        }
        Ok(())
    }

    /// Remove every session whose absolute deadline has passed.
    fn prune_sessions(&mut self, now_epoch_milliseconds: i64) {
        // preserve insertion order while discarding expired entries
        self.sessions
            .retain(|session| session.expires_at_epoch_milliseconds > now_epoch_milliseconds);
    }
}
