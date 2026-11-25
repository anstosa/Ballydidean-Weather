use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::process::Command;

use weather_edge::{
    AdmittedAdminMutation, AdmittedAdminOperator, DecodedForecastAdjustmentSettings,
    ForecastAdjustmentSettings, ForecastAdjustmentSettingsDiagnostic,
    ForecastAdjustmentSettingsOutcome, ForecastAdjustmentSettingsRequest,
    ForecastAdjustmentSettingsStatus, ForecastAdjustmentSettingsStore,
    InMemoryForecastAdjustmentSettingsStore, handle_forecast_adjustment_settings,
    validate_forecast_adjustment_settings,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FixtureStoreError;

impl Display for FixtureStoreError {
    /// render one ordinary injected persistence failure.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("write failed")
    }
}

impl Error for FixtureStoreError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FailingStore {
    inner: InMemoryForecastAdjustmentSettingsStore,
}

impl ForecastAdjustmentSettingsStore for FailingStore {
    type Error = FixtureStoreError;

    /// delegate reads to the unchanged in-memory snapshot.
    fn read_status(&mut self) -> ForecastAdjustmentSettingsStatus {
        self.inner.read_status()
    }

    /// fail before changing the in-memory snapshot.
    fn replace_atomically(
        &mut self,
        _settings: ForecastAdjustmentSettings,
    ) -> Result<ForecastAdjustmentSettings, Self::Error> {
        Err(FixtureStoreError)
    }
}

/// build one exact decoded four-field settings object.
fn candidate(temperature: bool, wind: bool, rain: bool) -> DecodedForecastAdjustmentSettings {
    DecodedForecastAdjustmentSettings {
        field_count: 4,
        version: Some(1),
        temperature: Some(temperature),
        wind: Some(wind),
        rain: Some(rain),
    }
}

/// render one response like the compiled Node semantic oracle.
fn outcome_signature(outcome: ForecastAdjustmentSettingsOutcome) -> String {
    // retain typed success, validation, persistence, and method outcomes
    match outcome {
        ForecastAdjustmentSettingsOutcome::Success {
            envelope,
            omit_body,
        } => {
            // omit only the serialized response body for HEAD
            let body = if omit_body {
                "head".to_owned()
            } else {
                format!(
                    "{}:{}{}{}",
                    envelope.data.version,
                    u8::from(envelope.data.temperature),
                    u8::from(envelope.data.wind),
                    u8::from(envelope.data.rain)
                )
            };
            let error = envelope
                .error
                .map(ForecastAdjustmentSettingsDiagnostic::wire_value)
                .unwrap_or("-");
            format!("{}:{body}:{error}:-", outcome.status_code())
        }
        ForecastAdjustmentSettingsOutcome::InvalidRequest => "400:invalid_request:-:-".to_owned(),
        ForecastAdjustmentSettingsOutcome::StoreFailure => {
            "500:internal_server_error:-:-".to_owned()
        }
        ForecastAdjustmentSettingsOutcome::MethodNotAllowed { allow } => format!(
            "405:method_not_allowed:-:{}",
            allow.header_value().replace(' ', "_")
        ),
    }
}

/// execute one admitted administrator read.
fn admin_read<Store: ForecastAdjustmentSettingsStore>(
    operator: &AdmittedAdminOperator,
    store: &mut Store,
    omit_body: bool,
) -> ForecastAdjustmentSettingsOutcome {
    handle_forecast_adjustment_settings(
        ForecastAdjustmentSettingsRequest::AdminRead {
            operator,
            omit_body,
        },
        store,
    )
}

// verifies actual source handler parity for read, update, recovery, and failures
#[test]
fn matches_compiled_node_admin_settings_paths() {
    let oracle =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/admin-settings-node-oracle.mjs");
    let output = Command::new("node")
        .arg(oracle)
        .output()
        .expect("run admin settings Node oracle");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let node = String::from_utf8(output.stdout).expect("Node oracle UTF-8");
    let operator = AdmittedAdminOperator::admitted("verified-operator");
    let mutation = AdmittedAdminMutation::admitted(&operator);
    let mut store = InMemoryForecastAdjustmentSettingsStore::configured_first_read();
    let read = admin_read(&operator, &mut store, false);
    let head = admin_read(&operator, &mut store, true);
    let update = handle_forecast_adjustment_settings(
        ForecastAdjustmentSettingsRequest::AdminReplace {
            mutation,
            input: candidate(true, false, false),
        },
        &mut store,
    );
    let read_after = admin_read(&operator, &mut store, false);
    let invalid = handle_forecast_adjustment_settings(
        ForecastAdjustmentSettingsRequest::AdminReplace {
            mutation,
            input: DecodedForecastAdjustmentSettings {
                field_count: 5,
                ..candidate(true, false, false)
            },
        },
        &mut store,
    );
    let read_after_invalid = admin_read(&operator, &mut store, false);
    let mut unavailable = InMemoryForecastAdjustmentSettingsStore::unavailable();
    let unavailable_admin = admin_read(&operator, &mut unavailable, false);
    let unavailable_public = handle_forecast_adjustment_settings(
        ForecastAdjustmentSettingsRequest::PublicRead { omit_body: false },
        &mut unavailable,
    );
    let unsupported = handle_forecast_adjustment_settings(
        ForecastAdjustmentSettingsRequest::AdminUnsupported {
            operator: &operator,
        },
        &mut store,
    );
    let mut failing = FailingStore {
        inner: InMemoryForecastAdjustmentSettingsStore::configured_first_read(),
    };
    let failed_write = handle_forecast_adjustment_settings(
        ForecastAdjustmentSettingsRequest::AdminReplace {
            mutation,
            input: candidate(true, false, false),
        },
        &mut failing,
    );
    let read_after_failed_write = admin_read(&operator, &mut failing, false);
    let rust = [
        ("read", read),
        ("head", head),
        ("update", update),
        ("read-after", read_after),
        ("invalid", invalid),
        ("read-after-invalid", read_after_invalid),
        ("unavailable-admin", unavailable_admin),
        ("unavailable-public", unavailable_public),
        ("unsupported", unsupported),
        ("failed-write", failed_write),
        ("read-after-failed-write", read_after_failed_write),
    ]
    .map(|(label, outcome)| format!("{label}={}", outcome_signature(outcome)))
    .join("\n");
    assert_eq!(rust, node.trim_end());
}

// verifies every malformed decoded shape fails before atomic replacement
#[test]
fn rejects_incomplete_or_unversioned_settings() {
    let cases = [
        DecodedForecastAdjustmentSettings {
            field_count: 3,
            ..candidate(true, true, true)
        },
        DecodedForecastAdjustmentSettings {
            version: Some(2),
            ..candidate(true, true, true)
        },
        DecodedForecastAdjustmentSettings {
            temperature: None,
            ..candidate(true, true, true)
        },
        DecodedForecastAdjustmentSettings {
            wind: None,
            ..candidate(true, true, true)
        },
        DecodedForecastAdjustmentSettings {
            rain: None,
            ..candidate(true, true, true)
        },
    ];

    // reject every malformed field family with the source error text
    for invalid in cases {
        let error = validate_forecast_adjustment_settings(invalid)
            .expect_err("invalid adjustment settings");
        assert_eq!(
            error.to_string(),
            "forecast adjustment settings are invalid"
        );
    }
}

// verifies an admitted update repairs fail-closed state atomically
#[test]
fn repairs_unavailable_state_only_after_valid_replacement() {
    let operator = AdmittedAdminOperator::admitted("verified-operator");
    let mutation = AdmittedAdminMutation::admitted(&operator);
    let mut store = InMemoryForecastAdjustmentSettingsStore::unavailable();
    let invalid = handle_forecast_adjustment_settings(
        ForecastAdjustmentSettingsRequest::AdminReplace {
            mutation,
            input: DecodedForecastAdjustmentSettings {
                field_count: 5,
                ..candidate(false, false, true)
            },
        },
        &mut store,
    );
    assert_eq!(invalid, ForecastAdjustmentSettingsOutcome::InvalidRequest);
    assert_eq!(
        admin_read(&operator, &mut store, false),
        ForecastAdjustmentSettingsOutcome::Success {
            envelope: weather_edge::ForecastAdjustmentSettingsEnvelope {
                data: weather_edge::DISABLED_FORECAST_ADJUSTMENT_SETTINGS,
                error: Some(ForecastAdjustmentSettingsDiagnostic::Unavailable),
            },
            omit_body: false,
        }
    );

    let saved = handle_forecast_adjustment_settings(
        ForecastAdjustmentSettingsRequest::AdminReplace {
            mutation,
            input: candidate(false, false, true),
        },
        &mut store,
    );
    assert_eq!(outcome_signature(saved), "200:1:001:-:-");
    assert_eq!(
        outcome_signature(admin_read(&operator, &mut store, false)),
        "200:1:001:-:-"
    );
}
