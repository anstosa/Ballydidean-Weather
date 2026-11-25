use std::error::Error;
use std::fmt::{Display, Formatter};

/// the fixed v1 settings returned when persistent state is unavailable.
pub const DISABLED_FORECAST_ADJUSTMENT_SETTINGS: ForecastAdjustmentSettings =
    ForecastAdjustmentSettings {
        version: 1,
        temperature: false,
        wind: false,
        rain: false,
    };

/// the fixed v1 settings created for one configured first read.
pub const DEFAULT_FORECAST_ADJUSTMENT_SETTINGS: ForecastAdjustmentSettings =
    ForecastAdjustmentSettings {
        version: 1,
        temperature: true,
        wind: true,
        rain: true,
    };

/// one administrator already verified by the owning session boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedAdminOperator(String);

impl AdmittedAdminOperator {
    /// admit one opaque operator identity after existing session verification.
    pub fn admitted(identity: impl Into<String>) -> Self {
        Self(identity.into())
    }
}

/// one admin mutation already admitted by session and same-origin checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdmittedAdminMutation<'a> {
    operator: &'a AdmittedAdminOperator,
}

impl<'a> AdmittedAdminMutation<'a> {
    /// admit one mutation after the owning handler verifies its origin.
    pub const fn admitted(operator: &'a AdmittedAdminOperator) -> Self {
        Self { operator }
    }
}

/// the complete v1 independent adjustment switch snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForecastAdjustmentSettings {
    pub version: u8,
    pub temperature: bool,
    pub wind: bool,
    pub rain: bool,
}

/// decoded object evidence supplied by the existing JSON adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodedForecastAdjustmentSettings {
    pub field_count: usize,
    pub version: Option<u64>,
    pub temperature: Option<bool>,
    pub wind: Option<bool>,
    pub rain: Option<bool>,
}

/// the one stable fail-closed diagnostic exposed only to administrators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastAdjustmentSettingsDiagnostic {
    Unavailable,
}

impl ForecastAdjustmentSettingsDiagnostic {
    /// return the exact admin response code from the Node handler.
    pub const fn wire_value(self) -> &'static str {
        "adjustment_settings_unavailable"
    }
}

/// one complete store read after default initialization or fail-closed recovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForecastAdjustmentSettingsStatus {
    pub settings: ForecastAdjustmentSettings,
    pub error: Option<ForecastAdjustmentSettingsDiagnostic>,
}

/// the response envelope before the existing JSON serializer runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForecastAdjustmentSettingsEnvelope {
    pub data: ForecastAdjustmentSettings,
    pub error: Option<ForecastAdjustmentSettingsDiagnostic>,
}

/// the exact method allow-list selected by public or admin mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastAdjustmentSettingsAllow {
    PublicRead,
    AdminReadWrite,
}

impl ForecastAdjustmentSettingsAllow {
    /// return the existing handler's exact Allow header value.
    pub const fn header_value(self) -> &'static str {
        // preserve the public and administrator route modes
        match self {
            Self::PublicRead => "GET, HEAD",
            Self::AdminReadWrite => "GET, HEAD, PUT",
        }
    }
}

/// one decoded request after transport, session, and origin admission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForecastAdjustmentSettingsRequest<'a> {
    PublicRead {
        omit_body: bool,
    },
    AdminRead {
        operator: &'a AdmittedAdminOperator,
        omit_body: bool,
    },
    AdminReplace {
        mutation: AdmittedAdminMutation<'a>,
        input: DecodedForecastAdjustmentSettings,
    },
    PublicUnsupported,
    AdminUnsupported {
        operator: &'a AdmittedAdminOperator,
    },
}

/// one typed handler result for the existing response adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastAdjustmentSettingsOutcome {
    Success {
        envelope: ForecastAdjustmentSettingsEnvelope,
        omit_body: bool,
    },
    InvalidRequest,
    StoreFailure,
    MethodNotAllowed {
        allow: ForecastAdjustmentSettingsAllow,
    },
}

impl ForecastAdjustmentSettingsOutcome {
    /// return the exact status selected by the Node handler.
    pub const fn status_code(self) -> u16 {
        // preserve success and ordinary handler failure categories
        match self {
            Self::Success { .. } => 200,
            Self::InvalidRequest => 400,
            Self::StoreFailure => 500,
            Self::MethodNotAllowed { .. } => 405,
        }
    }

    /// return the fixed no-store response policy.
    pub const fn cache_control(self) -> &'static str {
        "no-store"
    }
}

/// one exact decoded-input failure before any store mutation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForecastAdjustmentSettingsValidationError;

impl Display for ForecastAdjustmentSettingsValidationError {
    /// render the owning store's exact validation text.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("forecast adjustment settings are invalid")
    }
}

impl Error for ForecastAdjustmentSettingsValidationError {}

/// atomic settings storage injected by the owning runtime.
pub trait ForecastAdjustmentSettingsStore {
    type Error;

    /// read one initialized or fail-closed snapshot.
    fn read_status(&mut self) -> ForecastAdjustmentSettingsStatus;

    /// replace the complete four-field snapshot atomically.
    fn replace_atomically(
        &mut self,
        settings: ForecastAdjustmentSettings,
    ) -> Result<ForecastAdjustmentSettings, Self::Error>;
}

/// the three admitted states needed by the in-memory runtime adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InMemoryForecastAdjustmentSettingsState {
    ConfiguredFirstRead,
    Persisted(ForecastAdjustmentSettings),
    Unavailable,
}

/// an injected no-filesystem settings store for the Rust edge runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InMemoryForecastAdjustmentSettingsStore {
    state: InMemoryForecastAdjustmentSettingsState,
}

impl InMemoryForecastAdjustmentSettingsStore {
    /// create the configured first-read default state.
    pub const fn configured_first_read() -> Self {
        Self {
            state: InMemoryForecastAdjustmentSettingsState::ConfiguredFirstRead,
        }
    }

    /// create one already-persisted state.
    pub const fn persisted(settings: ForecastAdjustmentSettings) -> Self {
        Self {
            state: InMemoryForecastAdjustmentSettingsState::Persisted(settings),
        }
    }

    /// create one untrusted state that must fail closed.
    pub const fn unavailable() -> Self {
        Self {
            state: InMemoryForecastAdjustmentSettingsState::Unavailable,
        }
    }
}

impl ForecastAdjustmentSettingsStore for InMemoryForecastAdjustmentSettingsStore {
    type Error = std::convert::Infallible;

    /// initialize defaults once or return the fail-closed snapshot.
    fn read_status(&mut self) -> ForecastAdjustmentSettingsStatus {
        // preserve initialization, persisted state, and unavailable state
        match self.state {
            InMemoryForecastAdjustmentSettingsState::ConfiguredFirstRead => {
                self.state = InMemoryForecastAdjustmentSettingsState::Persisted(
                    DEFAULT_FORECAST_ADJUSTMENT_SETTINGS,
                );
                ForecastAdjustmentSettingsStatus {
                    settings: DEFAULT_FORECAST_ADJUSTMENT_SETTINGS,
                    error: None,
                }
            }
            InMemoryForecastAdjustmentSettingsState::Persisted(settings) => {
                ForecastAdjustmentSettingsStatus {
                    settings,
                    error: None,
                }
            }
            InMemoryForecastAdjustmentSettingsState::Unavailable => {
                ForecastAdjustmentSettingsStatus {
                    settings: DISABLED_FORECAST_ADJUSTMENT_SETTINGS,
                    error: Some(ForecastAdjustmentSettingsDiagnostic::Unavailable),
                }
            }
        }
    }

    /// atomically replace or repair the complete in-memory snapshot.
    fn replace_atomically(
        &mut self,
        settings: ForecastAdjustmentSettings,
    ) -> Result<ForecastAdjustmentSettings, Self::Error> {
        self.state = InMemoryForecastAdjustmentSettingsState::Persisted(settings);
        Ok(settings)
    }
}

/// execute the adjustment settings business policy over one injected store.
pub fn handle_forecast_adjustment_settings<Store: ForecastAdjustmentSettingsStore>(
    request: ForecastAdjustmentSettingsRequest<'_>,
    store: &mut Store,
) -> ForecastAdjustmentSettingsOutcome {
    // preserve public read, admin read, atomic update, and method modes
    match request {
        ForecastAdjustmentSettingsRequest::PublicRead { omit_body } => {
            let status = store.read_status();
            ForecastAdjustmentSettingsOutcome::Success {
                envelope: ForecastAdjustmentSettingsEnvelope {
                    data: status.settings,
                    error: None,
                },
                omit_body,
            }
        }
        ForecastAdjustmentSettingsRequest::AdminRead {
            operator,
            omit_body,
        } => {
            let _verified_operator = operator;
            let status = store.read_status();
            ForecastAdjustmentSettingsOutcome::Success {
                envelope: ForecastAdjustmentSettingsEnvelope {
                    data: status.settings,
                    error: status.error,
                },
                omit_body,
            }
        }
        ForecastAdjustmentSettingsRequest::AdminReplace { mutation, input } => {
            let _verified_operator = mutation.operator;
            let settings = match validate_forecast_adjustment_settings(input) {
                Ok(settings) => settings,
                Err(_) => return ForecastAdjustmentSettingsOutcome::InvalidRequest,
            };

            // expose the new snapshot only after atomic store success
            match store.replace_atomically(settings) {
                Ok(saved) => ForecastAdjustmentSettingsOutcome::Success {
                    envelope: ForecastAdjustmentSettingsEnvelope {
                        data: saved,
                        error: None,
                    },
                    omit_body: false,
                },
                Err(_) => ForecastAdjustmentSettingsOutcome::StoreFailure,
            }
        }
        ForecastAdjustmentSettingsRequest::PublicUnsupported => {
            ForecastAdjustmentSettingsOutcome::MethodNotAllowed {
                allow: ForecastAdjustmentSettingsAllow::PublicRead,
            }
        }
        ForecastAdjustmentSettingsRequest::AdminUnsupported { operator } => {
            let _verified_operator = operator;
            ForecastAdjustmentSettingsOutcome::MethodNotAllowed {
                allow: ForecastAdjustmentSettingsAllow::AdminReadWrite,
            }
        }
    }
}

/// validate the exact versioned four-field settings object.
pub fn validate_forecast_adjustment_settings(
    input: DecodedForecastAdjustmentSettings,
) -> Result<ForecastAdjustmentSettings, ForecastAdjustmentSettingsValidationError> {
    // reject missing, extra, unversioned, or non-boolean decoded fields
    if input.field_count != 4
        || input.version != Some(1)
        || input.temperature.is_none()
        || input.wind.is_none()
        || input.rain.is_none()
    {
        return Err(ForecastAdjustmentSettingsValidationError);
    }

    Ok(ForecastAdjustmentSettings {
        version: 1,
        temperature: input.temperature.expect("validated temperature field"),
        wind: input.wind.expect("validated wind field"),
        rain: input.rain.expect("validated rain field"),
    })
}
