use weather_contracts::{
    DueSource, JsonValue, Nullable, SiteConfiguration, SiteConfigurationProvider,
    SiteConfigurationSite, SiteConfigurationSource, SiteConfigurationStation, SourceCapability,
    SourceKind, StationKind,
};
use weather_runtime::{
    ConfiguredWorkerDueSourceProjection, WorkerDueSourceProjection, WorkerSourceMaterialIdentity,
};
use weather_worker::{
    OpenMeteoSourceKind, ScheduledSourceCatalogs, ScheduledSourceMaterialComparison, TaskFailure,
    WorkerDueSource,
};

struct Material;

impl ScheduledSourceMaterialComparison for Material {
    type Error = &'static str;

    /// Witness the exact already decoded source/catalog values passed to the approved owner.
    fn values_equal(&mut self, left: &JsonValue, right: &JsonValue) -> Result<bool, Self::Error> {
        Ok(left == right)
    }

    /// Compare the same object shape without synthesizing new material.
    fn value_and_object_equal(
        &mut self,
        value: &JsonValue,
        object: &[(Vec<u16>, JsonValue)],
    ) -> Result<bool, Self::Error> {
        Ok(matches!(value, JsonValue::Object(entries) if entries == object))
    }
}

impl WorkerSourceMaterialIdentity for Material {
    /// Simulate the available approved canonical identity without a real JSON codec.
    fn material_identity(&mut self, _: &JsonValue) -> Result<String, TaskFailure> {
        Ok("fixture-material-identity".to_owned())
    }
}

/// Build one decoded object carrying the exact declared adapter contract.
fn adapter_material(contract: &str) -> JsonValue {
    JsonValue::Object(vec![(
        "contractVersion".encode_utf16().collect(),
        JsonValue::String(contract.encode_utf16().collect()),
    )])
}

/// Admit a single normal virtual source matching the configured Open-Meteo runtime.
fn catalog(contract: &str) -> SiteConfiguration {
    SiteConfiguration {
        provider: SiteConfigurationProvider {
            active: true,
            attributionLabel: "OM".to_owned(),
            attributionUrl: String::new(),
            displayName: "Open-Meteo".to_owned(),
            key: "open-meteo".to_owned(),
        },
        site: SiteConfigurationSite {
            active: true,
            displayName: "Fixture".to_owned(),
            key: "fixture-site".to_owned(),
            latitude: 47.95,
            longitude: -122.43,
            timezone: "America/Los_Angeles".to_owned(),
        },
        station: SiteConfigurationStation {
            active: true,
            displayName: "Weather Station".to_owned(),
            key: "fixture-station".to_owned(),
            latitude: 47.95,
            longitude: -122.43,
            kind: StationKind::Virtual,
            model: Nullable::Null,
            serial: Nullable::Null,
            vendor: Nullable::Null,
        },
        sources: vec![SiteConfigurationSource {
            active: true,
            adapterConfig: adapter_material(contract),
            cadenceSeconds: Nullable::Value(60),
            capabilities: vec![SourceCapability::Current],
            fingerprint: "fixture-source".to_owned(),
            key: "current".to_owned(),
            sourceKind: SourceKind::ModelCurrent,
        }],
        version: 1,
    }
}

/// Represent one ordered decoded database result, not a provider response.
fn row(contract: &str) -> DueSource {
    DueSource {
        active: true,
        cadenceSeconds: 60,
        id: "42".to_owned(),
        materialProviderConfig: adapter_material(contract),
        providerKey: "open-meteo".to_owned(),
        siteSlug: "fixture-site".to_owned(),
        sourceConfigFingerprint: "fixture-source".to_owned(),
        sourceKey: "current".to_owned(),
        sourceKind: SourceKind::ModelCurrent,
        stationSlug: "fixture-station".to_owned(),
        timezone: "America/Los_Angeles".to_owned(),
    }
}

/// Wire one loaded configuration into the concrete projection used by the worker database.
fn projection(catalog: &SiteConfiguration) -> ConfiguredWorkerDueSourceProjection<'_, Material> {
    ConfiguredWorkerDueSourceProjection::from_catalogs(
        ScheduledSourceCatalogs {
            site: catalog,
            ecowitt: None,
            public_stations: None,
            tempest: None,
            tides: None,
        },
        Material,
    )
}

/// Prove actual catalog matching, skip, and pre-session adapter contract failure.
#[test]
fn generated_due_row_is_admitted_only_by_the_matching_loaded_catalog() {
    let configured = catalog("forecast-current/v1");
    let mut source_projection = projection(&configured);
    let valid = row("forecast-current/v1");
    let Some(WorkerDueSource::OpenMeteo(owned)) =
        source_projection.project_due_source(&valid).unwrap()
    else {
        panic!("matching source did not select the existing Open-Meteo runtime");
    };
    assert_eq!(owned.source_kind, OpenMeteoSourceKind::ModelCurrent);
    assert_eq!(owned.id, "42");
    assert_eq!(owned.adapter_version, "open-meteo-forecast-current/v1");
    assert_eq!(owned.material_config_identity, "fixture-material-identity");
    assert_eq!(
        source_projection.project_due_source(&row("forecast-current/v2")),
        Ok(None)
    );

    let mismatched_catalog = catalog("forecast-current/v2");
    let mut source_projection = projection(&mismatched_catalog);
    assert_eq!(
        source_projection.project_due_source(&row("forecast-current/v2")),
        Err(TaskFailure {
            attempts: 0,
            code: "source_adapter_contract_mismatch".to_owned(),
        })
    );
}
