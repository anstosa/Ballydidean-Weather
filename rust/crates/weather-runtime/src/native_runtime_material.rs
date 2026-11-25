//! Concrete value owners used by PostgreSQL bootstrap and worker/API source selection.

use std::{
    cmp::Ordering,
    fmt::{Display, Formatter},
};
use weather_contracts::{
    EcmwfTemperatureCanaryRecentErrorState, JsonValue, Nullable, SourceCapability,
};
use weather_database::{
    BootstrapJsonEncoder, MigrationArtifactNameOrder, MigrationArtifactSha256, MigrationDigest,
    SerializedJson, Sha256Digest,
};
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};
use weather_worker::{ScheduledSourceMaterialComparison, TaskFailure, TemperatureCanaryHour};

use crate::{
    NativeWirePrimitives, TemperatureCanaryPersistencePreparationPort,
    TemperatureCanaryRecentErrorDigestPort, TemperatureCanaryRunDigestMaterial,
    WorkerSourceMaterialIdentity,
};

/// One zero-state native JSON identity and digest owner shared across process roles.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeRuntimeMaterial;

/// A generated value that cannot be represented by the retained JSON contract.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeRuntimeMaterialError;

impl Display for NativeRuntimeMaterialError {
    /// Report the source's invalid canonical material category.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("invalid runtime JSON material")
    }
}

impl std::error::Error for NativeRuntimeMaterialError {}

impl crate::ApiMaintenanceCandidateCodec for NativeWirePrimitives {
    /// Validate the exact native canonical UTC witness before decoding an installed package.
    fn is_canonical_utc_instant(&mut self, value: &str) -> bool {
        self.canonical_utc_milliseconds(value).is_some()
    }
}

impl MigrationArtifactNameOrder for NativeRuntimeMaterial {
    /// Compare the restricted lowercase ASCII migration names using Node's observed collation.
    fn compare_names(&self, left: &str, right: &str) -> Ordering {
        // the admitted name grammar contains only digits lowercase letters and these separators
        left.bytes()
            .map(migration_collation_rank)
            .cmp(right.bytes().map(migration_collation_rank))
    }
}

/// Rank the complete admitted migration filename alphabet in the predecessor's locale order.
fn migration_collation_rank(value: u8) -> u8 {
    match value {
        b'_' => 0,
        b'.' => 1,
        value => value,
    }
}

impl MigrationArtifactSha256 for NativeRuntimeMaterial {
    /// Hash exactly the migration loader's selected UTF-8 bytes.
    fn sha256(&mut self, bytes: &[u8]) -> MigrationDigest {
        MigrationDigest::admitted(digest(bytes).as_str())
    }
}

impl BootstrapJsonEncoder for NativeRuntimeMaterial {
    type Error = NativeRuntimeMaterialError;

    /// Persist the original ordered object or primitive JSON without archive framing.
    fn value(&mut self, value: &JsonValue) -> Result<SerializedJson, Self::Error> {
        json_bytes(value, false)
            .and_then(|bytes| String::from_utf8(bytes).map_err(|_| NativeRuntimeMaterialError))
            .map(SerializedJson::admitted)
    }

    /// Encode the already projected ordered provider configuration.
    fn object(&mut self, entries: &[(Vec<u16>, JsonValue)]) -> Result<SerializedJson, Self::Error> {
        self.value(&JsonValue::Object(entries.to_vec()))
    }

    /// Retain the original capability array order and generated enum wire spellings.
    fn capabilities(&mut self, values: &[SourceCapability]) -> Result<SerializedJson, Self::Error> {
        let values = values
            .iter()
            .map(|value| {
                SourceCapability::WIRE_VALUES
                    .iter()
                    .find_map(|(wire, candidate)| (*candidate == *value).then(|| string(wire)))
                    .ok_or(NativeRuntimeMaterialError)
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.value(&JsonValue::Array(values))
    }
}

impl ScheduledSourceMaterialComparison for NativeRuntimeMaterial {
    type Error = NativeRuntimeMaterialError;

    /// Compare native canonical values rather than depending on JSONB property order.
    fn values_equal(&mut self, left: &JsonValue, right: &JsonValue) -> Result<bool, Self::Error> {
        Ok(json_bytes(left, true)? == json_bytes(right, true)?)
    }

    /// Compare one server object against the retained source projection.
    fn value_and_object_equal(
        &mut self,
        value: &JsonValue,
        object: &[(Vec<u16>, JsonValue)],
    ) -> Result<bool, Self::Error> {
        self.values_equal(value, &JsonValue::Object(object.to_vec()))
    }
}

impl WorkerSourceMaterialIdentity for NativeRuntimeMaterial {
    /// Retain one canonical JSON identity for equality between discovery and configured dispatch.
    fn material_identity(&mut self, value: &JsonValue) -> Result<String, TaskFailure> {
        json_bytes(value, true)
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .ok_or_else(material_failure)
    }
}

impl TemperatureCanaryRecentErrorDigestPort for NativeRuntimeMaterial {
    /// Hash the fixed-order Node database recent-error state with no trailing newline.
    fn digest_recent_error_state(
        &mut self,
        state: &EcmwfTemperatureCanaryRecentErrorState,
    ) -> Result<Sha256Digest, TaskFailure> {
        json_bytes(&recent_error_json(state), false)
            .map(|bytes| digest(&bytes))
            .map_err(|_| material_failure())
    }
}

impl TemperatureCanaryPersistencePreparationPort for NativeRuntimeMaterial {
    /// Parse explicit-zone inventory and provider clocks through the native date boundary.
    fn utc_epoch_milliseconds(&mut self, value: &str) -> Result<i64, TaskFailure> {
        NativeWirePrimitives
            .parse_explicit_utc_milliseconds(value)
            .ok_or_else(material_failure)
    }

    /// Produce the database owner’s exact millisecond UTC spelling.
    fn utc_string(&mut self, epoch_milliseconds: i64) -> Result<String, TaskFailure> {
        NativeWirePrimitives
            .format_utc_milliseconds(epoch_milliseconds)
            .ok_or_else(material_failure)
    }

    /// Serialize the same normalized recent-error object used by serving admission and hashing.
    fn serialize_recent_error_state(
        &mut self,
        state: &EcmwfTemperatureCanaryRecentErrorState,
    ) -> Result<SerializedJson, TaskFailure> {
        String::from_utf8(
            json_bytes(&recent_error_json(state), false).map_err(|_| material_failure())?,
        )
        .map(SerializedJson::admitted)
        .map_err(|_| material_failure())
    }

    /// Hash the exact normalized five-field database hour without an archive newline.
    fn digest_hour(&mut self, hour: &TemperatureCanaryHour) -> Result<Sha256Digest, TaskFailure> {
        let number = |value: Option<f64>| value.map_or(JsonValue::Null, JsonValue::Number);
        let material = JsonValue::Object(vec![
            member(
                "modelLeadHours",
                JsonValue::Number(f64::from(hour.model_lead_hours)),
            ),
            member(
                "rawRelativeHumidityPercent",
                number(hour.raw_relative_humidity_percent),
            ),
            member("rawTemperatureC", JsonValue::Number(hour.raw_temperature_c)),
            member("rawWindSpeedMps", number(hour.raw_wind_speed_mps)),
            member("validAt", string(&hour.valid_at)),
        ]);
        json_bytes(&material, false)
            .map(|bytes| digest(&bytes))
            .map_err(|_| material_failure())
    }

    /// Hash the original nine ordered immutable run fields, excluding the transport receipt clock.
    fn digest_run(
        &mut self,
        material: &TemperatureCanaryRunDigestMaterial,
    ) -> Result<Sha256Digest, TaskFailure> {
        let model_cycle = match material.model_cycle {
            weather_contracts::EcmwfTemperatureCanaryRunModelCycleEnum::Value49r1 => "49r1",
            weather_contracts::EcmwfTemperatureCanaryRunModelCycleEnum::Value50r1 => "50r1",
        };
        let status = match material.state_status {
            weather_contracts::EcmwfTemperatureCanaryRunStateStatusEnum::Supported => "supported",
            weather_contracts::EcmwfTemperatureCanaryRunStateStatusEnum::Cold => "cold",
            weather_contracts::EcmwfTemperatureCanaryRunStateStatusEnum::Insufficient => {
                "insufficient"
            }
            weather_contracts::EcmwfTemperatureCanaryRunStateStatusEnum::Invalid => "invalid",
        };
        let material = JsonValue::Object(vec![
            member(
                "adapterVersion",
                string(
                    weather_contracts::EcmwfTemperatureCanaryRunAdapterVersionLiteral::WIRE_VALUE,
                ),
            ),
            member(
                "hourContentHashes",
                JsonValue::Array(
                    material
                        .hour_content_hashes
                        .iter()
                        .map(|hash| string(hash.as_str()))
                        .collect(),
                ),
            ),
            member("modelCycle", string(model_cycle)),
            member(
                "providerResponseSha256",
                string(material.provider_response_sha256.as_str()),
            ),
            member(
                "recentErrorStateSha256",
                string(material.recent_error_state_sha256.as_str()),
            ),
            member("runInitializedAt", string(&material.run_initialized_at)),
            member("stateReason", string(&material.state_reason)),
            member("stateStatus", string(status)),
            member(
                "upstreamModel",
                string(
                    weather_contracts::EcmwfTemperatureCanaryRunUpstreamModelLiteral::WIRE_VALUE,
                ),
            ),
        ]);
        json_bytes(&material, false)
            .map(|bytes| digest(&bytes))
            .map_err(|_| material_failure())
    }
}

/// Render the database worker's exact fixed-order recent-error JSON object.
fn recent_error_json(state: &EcmwfTemperatureCanaryRecentErrorState) -> JsonValue {
    JsonValue::Object(vec![
        member("b24C", optional_number(&state.b24C)),
        member("b72C", optional_number(&state.b72C)),
        member(
            "cohort",
            string(
                weather_contracts::EcmwfTemperatureCanaryRecentErrorStateCohortLiteral::WIRE_VALUE,
            ),
        ),
        member("localDates", JsonValue::Number(state.localDates as f64)),
        member("mad72C", optional_number(&state.mad72C)),
        member(
            "maximumSourceRunInitializedAt",
            optional_text(&state.maximumSourceRunInitializedAt),
        ),
        member(
            "maximumSourceValidAt",
            optional_text(&state.maximumSourceValidAt),
        ),
        member("n24", JsonValue::Number(state.n24 as f64)),
        member("n72", JsonValue::Number(state.n72 as f64)),
        member(
            "sourceKeys",
            JsonValue::Array(state.sourceKeys.iter().map(|value| string(value)).collect()),
        ),
        member("supported", JsonValue::Boolean(state.supported)),
        member(
            "targetRunInitializedAt",
            string(&state.targetRunInitializedAt),
        ),
        member("windowEndValidAt", string(&state.windowEndValidAt)),
    ])
}

/// Construct one generated UTF-16 string.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Construct one named generated JSON member.
fn member(name: &str, value: JsonValue) -> (Vec<u16>, JsonValue) {
    (name.encode_utf16().collect(), value)
}

/// Preserve explicit null for the nullable source number.
fn optional_number(value: &Nullable<f64>) -> JsonValue {
    match value {
        Nullable::Value(value) => JsonValue::Number(*value),
        Nullable::Null => JsonValue::Null,
    }
}

/// Preserve explicit null for the nullable source string.
fn optional_text(value: &Nullable<String>) -> JsonValue {
    match value {
        Nullable::Value(value) => string(value),
        Nullable::Null => JsonValue::Null,
    }
}

/// Serialize one JSON value without the archive codec's final LF.
fn json_bytes(value: &JsonValue, sorted: bool) -> Result<Vec<u8>, NativeRuntimeMaterialError> {
    // preserve canonicalizeJson's refusal of nonfinite values
    if !finite_json(value) {
        return Err(NativeRuntimeMaterialError);
    }
    let mut bytes = if sorted {
        NativeWirePrimitives.encode_sorted_json(value)
    } else {
        NativeWirePrimitives.encode_ordered_json(value)
    }
    .ok_or(NativeRuntimeMaterialError)?;
    // strip only the codec's well-defined framing byte
    if bytes.pop() != Some(b'\n') {
        return Err(NativeRuntimeMaterialError);
    }
    Ok(bytes)
}

/// Validate the closed recursive JSON value domain for canonical material.
fn finite_json(value: &JsonValue) -> bool {
    match value {
        JsonValue::Number(value) => value.is_finite(),
        JsonValue::Array(values) => values.iter().all(finite_json),
        JsonValue::Object(fields) => fields.iter().all(|(_, value)| finite_json(value)),
        _ => true,
    }
}

/// Hash exactly the caller's bytes with the already approved native SHA owner.
fn digest(bytes: &[u8]) -> Sha256Digest {
    let hash = RevisionArchiveCanonicalCodec::sha256(&NativeWirePrimitives, bytes);
    let text = hash
        .iter()
        .fold(String::with_capacity(64), |mut text, byte| {
            use std::fmt::Write as _;
            let _ = write!(text, "{byte:02x}");
            text
        });
    Sha256Digest::admitted(text)
}

/// Return the stable source material failure code consumed by worker diagnostics.
fn material_failure() -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: "invalid_source_material".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Compare normalized hour and run identity with the predecessor JSON.stringify digest.
    #[test]
    fn temperature_canary_persistence_digests_match_node() {
        let mut owner = NativeRuntimeMaterial;
        let hour = TemperatureCanaryHour {
            model_lead_hours: 1,
            raw_relative_humidity_percent: None,
            raw_temperature_c: -0.0,
            raw_wind_speed_mps: Some(2.5),
            valid_at: "2026-11-24T13:00:00.000Z".to_owned(),
            valid_at_ms: 0,
        };
        let hour_hash = owner.digest_hour(&hour).unwrap();
        assert_eq!(
            hour_hash.as_str(),
            "b5e42e43f4e01ece861836faf1f8fc70e8296ef5ebf540b65b362ba120c61cf1"
        );
        let run = TemperatureCanaryRunDigestMaterial {
            adapter_version: weather_contracts::EcmwfTemperatureCanaryRunAdapterVersionLiteral,
            hour_content_hashes: vec![hour_hash],
            model_cycle: weather_contracts::EcmwfTemperatureCanaryRunModelCycleEnum::Value50r1,
            provider_response_sha256: Sha256Digest::admitted("a".repeat(64)),
            recent_error_state_sha256: Sha256Digest::admitted("b".repeat(64)),
            run_initialized_at: "2026-11-24T12:00:00.000Z".to_owned(),
            state_reason: "causal_recent_error_state_supported".to_owned(),
            state_status: weather_contracts::EcmwfTemperatureCanaryRunStateStatusEnum::Supported,
            upstream_model: weather_contracts::EcmwfTemperatureCanaryRunUpstreamModelLiteral,
        };
        assert_eq!(
            owner.digest_run(&run).unwrap().as_str(),
            "d5067bb890740b449eb6b24a88f63796bc9a38204ff0806914c863c5899ff7fd"
        );
    }

    /// Match Node's migration filename order for an intentionally mixed suffix catalog.
    #[test]
    fn canonical_migration_material_order() {
        let mut names = [
            "0001_a.sql",
            "0001_a_2.sql",
            "0001_a2.sql",
            "0001_a0.sql",
            "0001_a_z.sql",
        ];
        names.sort_by(|left, right| NativeRuntimeMaterial.compare_names(left, right));
        assert_eq!(
            names,
            [
                "0001_a_2.sql",
                "0001_a_z.sql",
                "0001_a.sql",
                "0001_a0.sql",
                "0001_a2.sql"
            ]
        );
        let mut owner = NativeRuntimeMaterial;
        let left = JsonValue::Object(vec![member("z", string("1")), member("a", string("2"))]);
        let right = JsonValue::Object(vec![member("a", string("2")), member("z", string("1"))]);
        assert!(owner.values_equal(&left, &right).unwrap());
        assert_eq!(
            owner.material_identity(&left).unwrap(),
            "{\"a\":\"2\",\"z\":\"1\"}"
        );
        assert_eq!(
            owner.value(&left).unwrap().as_str(),
            "{\"z\":\"1\",\"a\":\"2\"}"
        );
    }
}
