//! native process binding and ordered completion receipt for the migration entrypoint

use std::{
    env,
    ffi::OsString,
    fmt::{Display, Formatter},
    io::{self, Write},
};

use serde::{
    Serialize, Serializer,
    ser::{SerializeMap, SerializeStruct},
};
use weather_database::{
    BootstrapPublicStationReceipt, BootstrapSiteReceipt, BootstrapTideReceipt,
    CheckedMigrationResult, DatabaseProcessConfigurationError, MigrationDigest,
    MigrationSessionTimeouts, SensorBootstrapReceipt, load_current_process_database_configuration,
    to_pool_configuration,
};

use crate::{
    DEFAULT_ECOWITT_CONFIG_PATH, DEFAULT_MIGRATION_DIRECTORY, DEFAULT_PUBLIC_STATIONS_CONFIG_PATH,
    DEFAULT_SITE_CONFIG_PATH, DEFAULT_TEMPEST_CONFIG_PATH, DEFAULT_TIDE_CONFIG_PATH,
    MigrationBootstrapPool, MigrationBootstrapPoolError, MigrationCommandCompletion,
    MigrationCommandPaths, NativePostgresError, NativePostgresPool, NativeRuntimeMaterial,
    NativeRuntimeMaterialError, NativeWorkerConfigurationError, NativeWorkerJsonConfiguration,
    WEATHER_ECOWITT_CONFIG_PATH, WEATHER_MIGRATION_DIRECTORY, WEATHER_PUBLIC_STATIONS_CONFIG_PATH,
    WEATHER_SITE_CONFIG_PATH, WEATHER_TEMPEST_CONFIG_PATH, WEATHER_TIDE_CONFIG_PATH,
    parse_migration_command_arguments, run_checked_migration_command,
};

/// complete native migration command result before ordered JSON projection
pub type NativeMigrationCompletion = MigrationCommandCompletion<
    CheckedMigrationResult,
    BootstrapSiteReceipt,
    SensorBootstrapReceipt,
    SensorBootstrapReceipt,
    BootstrapTideReceipt,
    BootstrapPublicStationReceipt,
>;

/// complete retained migration and bootstrap failure for the native process
pub type NativeMigrationCommandError = MigrationBootstrapPoolError<
    NativePostgresError,
    NativeRuntimeMaterialError,
    NativeWorkerConfigurationError,
>;

/// bounded native migration process failure without database credentials
#[derive(Debug)]
pub enum NativeMigrationProcessError {
    InvalidArguments,
    Environment(&'static str),
    DatabaseConfiguration(DatabaseProcessConfigurationError),
    Pool(NativePostgresError),
    Command(NativeMigrationCommandError),
    Encode(serde_json::Error),
    Write(io::Error),
}

impl Display for NativeMigrationProcessError {
    /// render one stable process diagnostic without exposing configuration values
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        // preserve the owning process failure category
        match self {
            Self::InvalidArguments => {
                formatter.write_str("migration entrypoint arguments are invalid")
            }
            Self::Environment(name) => write!(formatter, "{name} could not be read"),
            Self::DatabaseConfiguration(error) => Display::fmt(error, formatter),
            Self::Pool(error) => Display::fmt(error, formatter),
            Self::Command(error) => write!(formatter, "migration command failed: {error:?}"),
            Self::Encode(_) => formatter.write_str("migration completion could not be encoded"),
            Self::Write(_) => formatter.write_str("migration completion could not be written"),
        }
    }
}

impl std::error::Error for NativeMigrationProcessError {
    /// expose only failures whose source diagnostics are already bounded
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        // retain typed causes where their contracts implement the error boundary
        match self {
            Self::DatabaseConfiguration(error) => Some(error),
            Self::Pool(error) => Some(error),
            Self::Encode(error) => Some(error),
            Self::Write(error) => Some(error),
            Self::InvalidArguments | Self::Environment(_) | Self::Command(_) => None,
        }
    }
}

/// owned current-process paths retained while the checked command borrows them
struct NativeMigrationPaths {
    migration_directory: String,
    site_configuration_path: String,
    ecowitt_configuration_path: String,
    tempest_configuration_path: String,
    tide_configuration_path: String,
    public_station_configuration_path: String,
}

impl NativeMigrationPaths {
    /// read the same exact environment keys and release-image defaults as the node entrypoint
    fn current() -> Result<Self, NativeMigrationProcessError> {
        Ok(Self {
            migration_directory: process_path(
                WEATHER_MIGRATION_DIRECTORY,
                DEFAULT_MIGRATION_DIRECTORY,
            )?,
            site_configuration_path: process_path(
                WEATHER_SITE_CONFIG_PATH,
                DEFAULT_SITE_CONFIG_PATH,
            )?,
            ecowitt_configuration_path: process_path(
                WEATHER_ECOWITT_CONFIG_PATH,
                DEFAULT_ECOWITT_CONFIG_PATH,
            )?,
            tempest_configuration_path: process_path(
                WEATHER_TEMPEST_CONFIG_PATH,
                DEFAULT_TEMPEST_CONFIG_PATH,
            )?,
            tide_configuration_path: process_path(
                WEATHER_TIDE_CONFIG_PATH,
                DEFAULT_TIDE_CONFIG_PATH,
            )?,
            public_station_configuration_path: process_path(
                WEATHER_PUBLIC_STATIONS_CONFIG_PATH,
                DEFAULT_PUBLIC_STATIONS_CONFIG_PATH,
            )?,
        })
    }

    /// borrow all owned paths for one complete migration lifecycle
    fn borrowed(&self) -> MigrationCommandPaths<'_> {
        MigrationCommandPaths {
            migration_directory: &self.migration_directory,
            site_configuration_path: &self.site_configuration_path,
            ecowitt_configuration_path: &self.ecowitt_configuration_path,
            tempest_configuration_path: &self.tempest_configuration_path,
            tide_configuration_path: &self.tide_configuration_path,
            public_station_configuration_path: &self.public_station_configuration_path,
        }
    }
}

/// run the actual one-shot native migration process and write exactly one completion line
pub fn run_native_migration_process(
    arguments: impl IntoIterator<Item = OsString>,
    output: &mut impl Write,
) -> Result<(), NativeMigrationProcessError> {
    // parse every argument before reading database configuration or opening a pool
    let mode = parse_process_arguments(arguments)?;
    let configuration = load_current_process_database_configuration()
        .map_err(NativeMigrationProcessError::DatabaseConfiguration)?;
    let timeouts = MigrationSessionTimeouts {
        lock_ms: configuration.lock_timeout_ms,
        statement_ms: configuration.statement_timeout_ms,
    };
    let paths = NativeMigrationPaths::current()?;
    let driver = NativePostgresPool::new(to_pool_configuration(&configuration))
        .map_err(NativeMigrationProcessError::Pool)?;
    let mut pool =
        MigrationBootstrapPool::new(driver, NativeWorkerJsonConfiguration, NativeRuntimeMaterial);
    let order = NativeRuntimeMaterial;
    let mut sha256 = NativeRuntimeMaterial;
    let completion = run_checked_migration_command(
        &mut pool,
        mode,
        paths.borrowed(),
        &order,
        &mut sha256,
        timeouts,
    )
    .map_err(NativeMigrationProcessError::Command)?;
    let mut receipt = encode_native_migration_completion(&completion)
        .map_err(NativeMigrationProcessError::Encode)?;
    receipt.push(b'\n');
    output
        .write_all(&receipt)
        .map_err(NativeMigrationProcessError::Write)
}

/// encode the exact source receipt property order without a trailing newline
pub fn encode_native_migration_completion(
    completion: &NativeMigrationCompletion,
) -> Result<Vec<u8>, serde_json::Error> {
    serde_json::to_vec(&MigrationCompletionOutput::from(completion))
}

/// decode process arguments without observing environment or filesystem state
fn parse_process_arguments(
    arguments: impl IntoIterator<Item = OsString>,
) -> Result<weather_database::MigrationMode, NativeMigrationProcessError> {
    let arguments = arguments
        .into_iter()
        .map(|argument| {
            argument
                .into_string()
                .map_err(|_| NativeMigrationProcessError::InvalidArguments)
        })
        .collect::<Result<Vec<_>, _>>()?;
    parse_migration_command_arguments(&arguments)
        .map_err(|_| NativeMigrationProcessError::InvalidArguments)
}

/// read one source path while preserving configured empty strings and absent defaults
fn process_path(
    name: &'static str,
    default: &'static str,
) -> Result<String, NativeMigrationProcessError> {
    // distinguish a missing value from an invalid nonunicode process value
    match env::var(name) {
        Ok(value) => Ok(value),
        Err(env::VarError::NotPresent) => Ok(default.to_owned()),
        Err(env::VarError::NotUnicode(_)) => Err(NativeMigrationProcessError::Environment(name)),
    }
}

/// insertion-ordered javascript record projection with last-value replacement
struct JavaScriptRecord<'a>(Vec<(&'a str, &'a str)>);

impl<'a> JavaScriptRecord<'a> {
    /// project record assignments while retaining the first insertion position
    fn from_pairs(pairs: &'a [(String, String)]) -> Self {
        let mut entries = Vec::<(&str, &str)>::with_capacity(pairs.len());
        // apply each source record assignment in declaration order
        for (key, value) in pairs {
            // replace an existing value without moving its property position
            if let Some(entry) = entries.iter_mut().find(|entry| entry.0 == key) {
                entry.1 = value;
            } else {
                entries.push((key, value));
            }
        }
        Self(entries)
    }
}

impl Serialize for JavaScriptRecord<'_> {
    /// serialize the retained insertion order as one json object
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        // emit every already normalized property exactly once
        for (key, value) in &self.0 {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

/// exact base-site receipt property order
struct SiteBootstrapOutput<'a> {
    provider_id: &'a str,
    site_id: &'a str,
    source_ids: &'a [String],
    station_id: &'a str,
}

impl Serialize for SiteBootstrapOutput<'_> {
    /// serialize the exact base site receipt order
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut receipt = serializer.serialize_struct("BootstrapSiteReceipt", 4)?;
        receipt.serialize_field("providerId", self.provider_id)?;
        receipt.serialize_field("siteId", self.site_id)?;
        receipt.serialize_field("sourceIds", self.source_ids)?;
        receipt.serialize_field("stationId", self.station_id)?;
        receipt.end()
    }
}

/// exact physical sensor receipt property order
struct SensorBootstrapOutput<'a> {
    provider_id: &'a str,
    source_ids: JavaScriptRecord<'a>,
    station_ids: JavaScriptRecord<'a>,
}

impl Serialize for SensorBootstrapOutput<'_> {
    /// serialize one physical sensor receipt in source order
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut receipt = serializer.serialize_struct("SensorBootstrapReceipt", 3)?;
        receipt.serialize_field("providerId", self.provider_id)?;
        receipt.serialize_field("sourceIds", &self.source_ids)?;
        receipt.serialize_field("stationIds", &self.station_ids)?;
        receipt.end()
    }
}

impl<'a> SensorBootstrapOutput<'a> {
    /// borrow one retained physical sensor receipt
    fn from(receipt: &'a SensorBootstrapReceipt) -> Self {
        Self {
            provider_id: &receipt.provider_id,
            source_ids: JavaScriptRecord::from_pairs(&receipt.source_ids),
            station_ids: JavaScriptRecord::from_pairs(&receipt.station_ids),
        }
    }
}

/// exact public station receipt property order
struct PublicStationBootstrapOutput<'a> {
    provider_ids: JavaScriptRecord<'a>,
    source_ids: JavaScriptRecord<'a>,
    station_ids: JavaScriptRecord<'a>,
}

impl Serialize for PublicStationBootstrapOutput<'_> {
    /// serialize one public station receipt in source order
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut receipt = serializer.serialize_struct("BootstrapPublicStationReceipt", 3)?;
        receipt.serialize_field("providerIds", &self.provider_ids)?;
        receipt.serialize_field("sourceIds", &self.source_ids)?;
        receipt.serialize_field("stationIds", &self.station_ids)?;
        receipt.end()
    }
}

/// exact migration completion property order after bootstrap receipts
struct MigrationCompletionOutput<'a> {
    bootstrap: SiteBootstrapOutput<'a>,
    ecowitt_bootstrap: SensorBootstrapOutput<'a>,
    event: &'static str,
    public_station_bootstrap: PublicStationBootstrapOutput<'a>,
    tempest_bootstrap: SensorBootstrapOutput<'a>,
    tide_bootstrap: SensorBootstrapOutput<'a>,
    applied: &'a [String],
    current: &'a [String],
    execution_variant: Option<&'a str>,
    history_sha256: Option<&'a str>,
    revised_20_sha256: Option<&'a str>,
    revised_23_sha256: Option<&'a str>,
    server_version_num: u64,
}

impl Serialize for MigrationCompletionOutput<'_> {
    /// serialize the full completion in object spread insertion order
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let optional_fields = usize::from(self.execution_variant.is_some())
            + usize::from(self.history_sha256.is_some())
            + usize::from(self.revised_20_sha256.is_some())
            + usize::from(self.revised_23_sha256.is_some());
        let mut receipt =
            serializer.serialize_struct("MigrationCompletion", 9 + optional_fields)?;
        receipt.serialize_field("bootstrap", &self.bootstrap)?;
        receipt.serialize_field("ecowittBootstrap", &self.ecowitt_bootstrap)?;
        receipt.serialize_field("event", self.event)?;
        receipt.serialize_field("publicStationBootstrap", &self.public_station_bootstrap)?;
        receipt.serialize_field("tempestBootstrap", &self.tempest_bootstrap)?;
        receipt.serialize_field("tideBootstrap", &self.tide_bootstrap)?;
        receipt.serialize_field("applied", self.applied)?;
        receipt.serialize_field("current", self.current)?;
        // retain only direct atomic evidence supplied by the checked migration owner
        if let Some(value) = self.execution_variant {
            receipt.serialize_field("executionVariant", value)?;
        }
        // retain the exact reviewed history digest when present
        if let Some(value) = self.history_sha256 {
            receipt.serialize_field("historySha256", value)?;
        }
        // retain the revised frontier digest when present
        if let Some(value) = self.revised_20_sha256 {
            receipt.serialize_field("revised20Sha256", value)?;
        }
        // retain the revised index digest when present
        if let Some(value) = self.revised_23_sha256 {
            receipt.serialize_field("revised23Sha256", value)?;
        }
        receipt.serialize_field("serverVersionNum", &self.server_version_num)?;
        receipt.end()
    }
}

impl<'a> MigrationCompletionOutput<'a> {
    /// project the checked result and exact bootstrap receipts into source order
    fn from(completion: &'a NativeMigrationCompletion) -> Self {
        let evidence = completion.migrations.evidence.as_ref();
        Self {
            bootstrap: SiteBootstrapOutput {
                provider_id: &completion.bootstrap.provider_id,
                site_id: &completion.bootstrap.site_id,
                source_ids: &completion.bootstrap.source_ids,
                station_id: &completion.bootstrap.station_id,
            },
            ecowitt_bootstrap: SensorBootstrapOutput::from(&completion.ecowitt_bootstrap),
            event: NativeMigrationCompletion::EVENT,
            public_station_bootstrap: PublicStationBootstrapOutput {
                provider_ids: JavaScriptRecord::from_pairs(
                    &completion.public_station_bootstrap.provider_ids,
                ),
                source_ids: JavaScriptRecord::from_pairs(
                    &completion.public_station_bootstrap.source_ids,
                ),
                station_ids: JavaScriptRecord::from_pairs(
                    &completion.public_station_bootstrap.station_ids,
                ),
            },
            tempest_bootstrap: SensorBootstrapOutput::from(&completion.tempest_bootstrap),
            tide_bootstrap: SensorBootstrapOutput {
                provider_id: &completion.tide_bootstrap.provider_id,
                source_ids: JavaScriptRecord::from_pairs(&completion.tide_bootstrap.source_ids),
                station_ids: JavaScriptRecord::from_pairs(&completion.tide_bootstrap.station_ids),
            },
            applied: &completion.migrations.migrations.applied,
            current: &completion.migrations.migrations.current,
            execution_variant: evidence.map(|evidence| evidence.execution_variant),
            history_sha256: evidence.and_then(|evidence| digest(&evidence.history_sha256)),
            revised_20_sha256: evidence.and_then(|evidence| digest(&evidence.revised_20_sha256)),
            revised_23_sha256: evidence.and_then(|evidence| digest(&evidence.revised_23_sha256)),
            server_version_num: completion.migrations.server_version_num,
        }
    }
}

/// borrow one optional admitted migration digest
fn digest(value: &Option<MigrationDigest>) -> Option<&str> {
    value.as_ref().map(MigrationDigest::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;
    use weather_database::AtomicMigrationEvidence;
    use weather_database::MigrationExecutionResult;

    /// build one complete receipt fixture without opening a database
    fn completion(evidence: Option<AtomicMigrationEvidence>) -> NativeMigrationCompletion {
        MigrationCommandCompletion {
            migrations: CheckedMigrationResult {
                migrations: MigrationExecutionResult {
                    applied: vec!["0002_next.sql".to_owned()],
                    current: vec!["0001_first.sql".to_owned()],
                },
                server_version_num: 170_010,
                evidence,
            },
            bootstrap: BootstrapSiteReceipt {
                provider_id: "provider".to_owned(),
                site_id: "site".to_owned(),
                source_ids: vec!["source".to_owned()],
                station_id: "station".to_owned(),
            },
            ecowitt_bootstrap: sensor("ecowitt"),
            tempest_bootstrap: sensor("tempest"),
            tide_bootstrap: BootstrapTideReceipt {
                provider_id: "tide-provider".to_owned(),
                source_ids: vec![("tide-source".to_owned(), "31".to_owned())],
                station_ids: vec![("tide-station".to_owned(), "32".to_owned())],
            },
            public_station_bootstrap: BootstrapPublicStationReceipt {
                provider_ids: vec![("public-provider".to_owned(), "41".to_owned())],
                source_ids: vec![("public-source".to_owned(), "42".to_owned())],
                station_ids: vec![("public-station".to_owned(), "43".to_owned())],
            },
        }
    }

    /// build one ordered sensor receipt and exercise javascript record replacement
    fn sensor(prefix: &str) -> SensorBootstrapReceipt {
        SensorBootstrapReceipt {
            provider_id: format!("{prefix}-provider"),
            source_ids: vec![
                (format!("{prefix}-source"), "11".to_owned()),
                (format!("{prefix}-source"), "12".to_owned()),
            ],
            station_ids: vec![(format!("{prefix}-station"), "13".to_owned())],
        }
    }

    #[test]
    /// match the ordinary source receipt byte for byte
    fn ordinary_completion_matches_source_property_order() {
        let bytes = encode_native_migration_completion(&completion(None)).expect("encode receipt");
        let text = String::from_utf8(bytes).expect("utf8 receipt");
        assert_eq!(
            text,
            concat!(
                "{\"bootstrap\":{\"providerId\":\"provider\",\"siteId\":\"site\",",
                "\"sourceIds\":[\"source\"],\"stationId\":\"station\"},",
                "\"ecowittBootstrap\":{\"providerId\":\"ecowitt-provider\",",
                "\"sourceIds\":{\"ecowitt-source\":\"12\"},",
                "\"stationIds\":{\"ecowitt-station\":\"13\"}},",
                "\"event\":\"migrations_complete\",",
                "\"publicStationBootstrap\":{\"providerIds\":{\"public-provider\":\"41\"},",
                "\"sourceIds\":{\"public-source\":\"42\"},",
                "\"stationIds\":{\"public-station\":\"43\"}},",
                "\"tempestBootstrap\":{\"providerId\":\"tempest-provider\",",
                "\"sourceIds\":{\"tempest-source\":\"12\"},",
                "\"stationIds\":{\"tempest-station\":\"13\"}},",
                "\"tideBootstrap\":{\"providerId\":\"tide-provider\",",
                "\"sourceIds\":{\"tide-source\":\"31\"},",
                "\"stationIds\":{\"tide-station\":\"32\"}},",
                "\"applied\":[\"0002_next.sql\"],\"current\":[\"0001_first.sql\"],",
                "\"serverVersionNum\":170010}"
            )
        );
    }

    #[test]
    /// retain the exact optional atomic evidence insertion point
    fn atomic_completion_inserts_only_present_evidence_before_server_version() {
        let receipt = completion(Some(AtomicMigrationEvidence {
            execution_variant: "atomic-direct",
            history_sha256: Some(MigrationDigest::admitted("history")),
            revised_20_sha256: Some(MigrationDigest::admitted("revision-20")),
            revised_23_sha256: None,
        }));
        let bytes = encode_native_migration_completion(&receipt).expect("encode receipt");
        let text = String::from_utf8(bytes).expect("utf8 receipt");
        assert!(text.ends_with(concat!(
            "\"applied\":[\"0002_next.sql\"],\"current\":[\"0001_first.sql\"],",
            "\"executionVariant\":\"atomic-direct\",\"historySha256\":\"history\",",
            "\"revised20Sha256\":\"revision-20\",\"serverVersionNum\":170010}"
        )));
        assert!(!text.contains("revised23Sha256"));
    }

    #[test]
    /// reject unsupported operands without reading process configuration
    fn invalid_arguments_fail_before_any_process_configuration_is_needed() {
        assert!(matches!(
            parse_process_arguments([OsString::from("--unknown")]),
            Err(NativeMigrationProcessError::InvalidArguments)
        ));
        assert!(matches!(
            parse_process_arguments([
                OsString::from("--atomic-maintenance-v14"),
                OsString::from("extra"),
            ]),
            Err(NativeMigrationProcessError::InvalidArguments)
        ));
    }
}
