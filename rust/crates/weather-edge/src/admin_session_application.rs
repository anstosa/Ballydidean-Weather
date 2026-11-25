use crate::{
    AdmittedAdminCredential, AdmittedAdminPersistentAuthState, AdmittedAdminSessionToken,
    WeatherAdminAuthStatePort, WeatherAdminBootstrapStatus, WeatherAdminSessionClock,
    WeatherAdminSessionMaterialPort, WeatherAdminSessionResponse, WeatherAdminStore,
    WeatherAdminStoreError,
};

/// cookie attribute mode admitted from the trusted transport context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmittedAdminCookieMode {
    SameSiteLax,
    SecurePartitioned,
}

/// one login-page query state admitted by the existing URL adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdmittedAdminLoginPage {
    pub invalid_credentials: bool,
}

/// one route-specific input after form, cookie, credential, and origin admission.
#[derive(Debug)]
pub enum AdmittedAdminRouteInput<'a> {
    Bootstrap(AdmittedAdminPersistentAuthState),
    Login {
        cookie_mode: AdmittedAdminCookieMode,
        credential: AdmittedAdminCredential,
    },
    Logout {
        cookie_mode: AdmittedAdminCookieMode,
        token: Option<&'a AdmittedAdminSessionToken>,
    },
    Page {
        login: AdmittedAdminLoginPage,
        token: Option<&'a AdmittedAdminSessionToken>,
    },
}

/// one home-network decision admitted by the existing matcher owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdmittedViewerContext {
    pub home_network: bool,
}

/// exact route method advertisement retained before HTTP serialization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeatherAdminRouteAllow {
    Post,
}

impl WeatherAdminRouteAllow {
    /// return the source handler's exact Allow header value.
    pub const fn header_value(self) -> &'static str {
        "POST"
    }
}

/// one typed administrator route result before cookie or response serialization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WeatherAdminRouteOutcome {
    BootstrapConfigured,
    BootstrapAlreadyConfigured,
    LoginRedirect {
        cookie_mode: AdmittedAdminCookieMode,
        session: WeatherAdminSessionResponse,
    },
    LoginInvalidRedirect,
    LogoutRedirect {
        cookie_mode: AdmittedAdminCookieMode,
    },
    MethodNotAllowed {
        allow: WeatherAdminRouteAllow,
    },
    StoreFailure,
}

impl WeatherAdminRouteOutcome {
    /// return the exact source status code selected for this outcome.
    pub const fn status_code(&self) -> u16 {
        // preserve configured, redirect, conflict, method, and store results
        match self {
            Self::BootstrapConfigured => 201,
            Self::BootstrapAlreadyConfigured => 409,
            Self::LoginRedirect { .. }
            | Self::LoginInvalidRedirect
            | Self::LogoutRedirect { .. } => 303,
            Self::MethodNotAllowed { .. } => 405,
            Self::StoreFailure => 500,
        }
    }

    /// return the exact redirect target for redirect outcomes.
    pub const fn location(&self) -> Option<&'static str> {
        // retain the framed invalid-login redirect separately from normal admin entry
        match self {
            Self::LoginInvalidRedirect => Some("/admin?error=invalid"),
            Self::LoginRedirect { .. } | Self::LogoutRedirect { .. } => Some("/admin"),
            Self::BootstrapConfigured
            | Self::BootstrapAlreadyConfigured
            | Self::MethodNotAllowed { .. }
            | Self::StoreFailure => None,
        }
    }

    /// return the no-store response policy shared by these routes.
    pub const fn cache_control(&self) -> &'static str {
        "no-store"
    }
}

/// one typed viewer-context result before canonical JSON serialization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeatherViewerContextOutcome {
    Success { home_network: bool, omit_body: bool },
    MethodNotAllowed,
}

impl WeatherViewerContextOutcome {
    /// return the exact source status selected for this outcome.
    pub const fn status_code(self) -> u16 {
        // preserve the read success and method rejection categories
        match self {
            Self::Success { .. } => 200,
            Self::MethodNotAllowed => 405,
        }
    }

    /// return the source route's private response cache policy.
    pub const fn cache_control(self) -> &'static str {
        // retain private context only for the successful viewer response
        match self {
            Self::Success { .. } => "private, no-store",
            Self::MethodNotAllowed => "no-store",
        }
    }

    /// return the optional read-only method advertisement.
    pub const fn allow(self) -> Option<&'static str> {
        // attach the source Allow header only to method rejection
        match self {
            Self::Success { .. } => None,
            Self::MethodNotAllowed => Some("GET, HEAD"),
        }
    }
}

/// application-level ownership of the real in-process administrator session lifecycle.
pub trait WeatherAdminSessionApplicationPort {
    /// persist the first already-verified credential state.
    fn bootstrap(
        &mut self,
        state: AdmittedAdminPersistentAuthState,
    ) -> Result<WeatherAdminBootstrapStatus, WeatherAdminStoreError>;

    /// exchange one already-verified credential for an opaque session.
    fn login(
        &mut self,
        credential: &AdmittedAdminCredential,
    ) -> Result<Option<WeatherAdminSessionResponse>, WeatherAdminStoreError>;

    /// revoke one optional admitted browser session.
    fn logout(
        &mut self,
        token: Option<&AdmittedAdminSessionToken>,
    ) -> Result<(), WeatherAdminStoreError>;

    /// authenticate one admitted browser session for a private representation.
    fn authenticate(
        &mut self,
        token: &AdmittedAdminSessionToken,
    ) -> Result<bool, WeatherAdminStoreError>;
}

/// concrete adapter joining the dispatcher to the landed administrator store ports.
pub struct WeatherAdminSessionApplication<'a, Auth, Clock, Material> {
    auth: &'a mut Auth,
    clock: &'a mut Clock,
    material: &'a mut Material,
    store: &'a mut WeatherAdminStore,
}

impl<'a, Auth, Clock, Material> WeatherAdminSessionApplication<'a, Auth, Clock, Material> {
    /// bind one request scope to its in-process store and admitted external ports.
    pub fn new(
        store: &'a mut WeatherAdminStore,
        auth: &'a mut Auth,
        clock: &'a mut Clock,
        material: &'a mut Material,
    ) -> Self {
        Self {
            auth,
            clock,
            material,
            store,
        }
    }
}

impl<Auth, Clock, Material> WeatherAdminSessionApplicationPort
    for WeatherAdminSessionApplication<'_, Auth, Clock, Material>
where
    Auth: WeatherAdminAuthStatePort,
    Clock: WeatherAdminSessionClock,
    Material: WeatherAdminSessionMaterialPort,
{
    /// delegate one admitted bootstrap to the real store.
    fn bootstrap(
        &mut self,
        state: AdmittedAdminPersistentAuthState,
    ) -> Result<WeatherAdminBootstrapStatus, WeatherAdminStoreError> {
        self.store
            .bootstrap(self.auth, state)
            .map(|response| response.status)
    }

    /// delegate one admitted login to the real store.
    fn login(
        &mut self,
        credential: &AdmittedAdminCredential,
    ) -> Result<Option<WeatherAdminSessionResponse>, WeatherAdminStoreError> {
        self.store
            .start_session(self.auth, self.clock, self.material, credential)
    }

    /// delegate one optional admitted logout to the real store.
    fn logout(
        &mut self,
        token: Option<&AdmittedAdminSessionToken>,
    ) -> Result<(), WeatherAdminStoreError> {
        // preserve the source no-op for an absent browser session cookie
        if let Some(token) = token {
            self.store.revoke_session(self.material, token)?;
        }
        Ok(())
    }

    /// delegate one admitted page session read to the real store.
    fn authenticate(
        &mut self,
        token: &AdmittedAdminSessionToken,
    ) -> Result<bool, WeatherAdminStoreError> {
        self.store
            .authenticate_session(self.clock, self.material, token)
    }
}

/// execute one admitted bootstrap request through the real session application port.
pub fn handle_admin_bootstrap<Port: WeatherAdminSessionApplicationPort>(
    state: AdmittedAdminPersistentAuthState,
    port: &mut Port,
) -> WeatherAdminRouteOutcome {
    // preserve configured, already-configured, and persistence outcomes
    match port.bootstrap(state) {
        Ok(WeatherAdminBootstrapStatus::Configured) => {
            WeatherAdminRouteOutcome::BootstrapConfigured
        }
        Ok(WeatherAdminBootstrapStatus::AlreadyConfigured) => {
            WeatherAdminRouteOutcome::BootstrapAlreadyConfigured
        }
        Err(_) => WeatherAdminRouteOutcome::StoreFailure,
    }
}

/// execute one admitted login request through the real session application port.
pub fn handle_admin_login<Port: WeatherAdminSessionApplicationPort>(
    credential: &AdmittedAdminCredential,
    cookie_mode: AdmittedAdminCookieMode,
    port: &mut Port,
) -> WeatherAdminRouteOutcome {
    // preserve successful and stale-proof redirects plus ordinary store failure
    match port.login(credential) {
        Ok(Some(session)) => WeatherAdminRouteOutcome::LoginRedirect {
            cookie_mode,
            session,
        },
        Ok(None) => WeatherAdminRouteOutcome::LoginInvalidRedirect,
        Err(_) => WeatherAdminRouteOutcome::StoreFailure,
    }
}

/// execute one admitted logout request through the real session application port.
pub fn handle_admin_logout<Port: WeatherAdminSessionApplicationPort>(
    token: Option<&AdmittedAdminSessionToken>,
    cookie_mode: AdmittedAdminCookieMode,
    port: &mut Port,
) -> WeatherAdminRouteOutcome {
    // clear the browser cookie only after the in-process revocation path completes
    match port.logout(token) {
        Ok(()) => WeatherAdminRouteOutcome::LogoutRedirect { cookie_mode },
        Err(_) => WeatherAdminRouteOutcome::StoreFailure,
    }
}
