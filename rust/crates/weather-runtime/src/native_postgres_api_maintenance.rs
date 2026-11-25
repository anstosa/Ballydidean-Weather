//! Native PostgreSQL execution for private API maintenance reads and compact writes.

use std::{sync::mpsc, thread};

use weather_contracts::{
    AdjustmentRevisionCommitReceipt, AdjustmentRevisionGapReason, AdjustmentRevisionProjectionKind,
    AdjustmentShadowFamily, AdjustmentShadowPredictionReceipt, AdjustmentShadowRegistrationReceipt,
    AdjustmentShadowRegistrationSlot, AdjustmentShadowRegistrationSlotContractVersionLiteral,
    AdjustmentShadowRegistrationSlotStateEnum, AdjustmentWeatherRevisionPointerSourceKindEnum,
    JsonValue, Nullable,
};
use weather_database::{
    AdjustmentMaintenanceDatabasePort, AdjustmentRevisionAdmissionKey, AdjustmentRevisionGapMarker,
    AdjustmentShadowRegistrationSlotBind, AdjustmentShadowRegistrationSlotQueryPlan,
    ApiMaintenanceJsonBind, ApiMaintenancePredictionPlan, ApiMaintenancePredictionWritePort,
    ApiMaintenanceRegistrationPlan, MaintenancePublicationBind, MaintenancePublicationPlan,
};
use weather_edge::{ArchiveWireCodec, RevisionArchiveCanonicalCodec};

use crate::{
    NativePostgresError, NativePostgresParameter as Parameter, NativePostgresPool,
    NativePostgresSession, NativeWirePrimitives, WeatherApiMaintenancePool,
    WeatherApiMaintenanceWritePool, WeatherApiReadPool, api_read_pool::run_api_pool_query,
    native_json::NativeJson, native_postgres_weather_rows as field,
};

const MAXIMUM_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
const MAXIMUM_REGISTRATION_SLOT_BYTES: usize = 2_048;

impl AdjustmentMaintenanceDatabasePort for NativePostgresSession {
    type Error = NativePostgresError;

    /// Execute one family slot facade and decode its exact generated result.
    fn read_schedule_slot(
        &mut self,
        plan: AdjustmentShadowRegistrationSlotQueryPlan,
    ) -> Result<Vec<AdjustmentShadowRegistrationSlot>, Self::Error> {
        let [AdjustmentShadowRegistrationSlotBind::Family(family)] = plan.binds;
        let parameters = [Parameter::Text(wire_value(
            family,
            AdjustmentShadowFamily::WIRE_VALUES,
            "adjustment family",
        )?)];
        query_json_rows(self, plan.sql, &parameters, "value", |value| {
            decode_registration_slot(value, family)
        })
    }

    /// Execute the scalar body-admission facade without accepting truthy aliases.
    fn read_shadow_body_admission(
        &mut self,
        plan: MaintenancePublicationPlan<'_, 4>,
    ) -> Result<Vec<bool>, Self::Error> {
        let parameters = publication_parameters(&plan.binds)?;
        self.query(plan.sql, &parameters)?
            .iter()
            .map(|row| field::column(row, "admitted"))
            .collect()
    }

    /// Decode one authenticated shadow revision receipt from the server JSON value.
    fn read_shadow_revision_receipt(
        &mut self,
        plan: MaintenancePublicationPlan<'_, 4>,
    ) -> Result<Vec<AdjustmentRevisionCommitReceipt>, Self::Error> {
        let parameters = publication_parameters(&plan.binds)?;
        query_json_rows(self, plan.sql, &parameters, "value", decode_commit_receipt)
    }

    /// Decode one authenticated current-pointer receipt from the server JSON value.
    fn read_current_revision_receipt(
        &mut self,
        plan: MaintenancePublicationPlan<'_, 1>,
    ) -> Result<Vec<AdjustmentRevisionCommitReceipt>, Self::Error> {
        let parameters = publication_parameters(&plan.binds)?;
        query_json_rows(self, plan.sql, &parameters, "value", decode_commit_receipt)
    }

    /// Write and decode one exact permanent revision-gap marker.
    fn write_revision_gap(
        &mut self,
        plan: MaintenancePublicationPlan<'_, 3>,
    ) -> Result<Vec<AdjustmentRevisionGapMarker>, Self::Error> {
        let parameters = publication_parameters(&plan.binds)?;
        query_json_rows(
            self,
            plan.sql,
            &parameters,
            "value",
            decode_revision_gap_marker,
        )
    }
}

impl ApiMaintenancePredictionWritePort for NativePostgresSession {
    type Error = NativePostgresError;

    /// Execute one versioned registration and decode its exact compact receipt.
    fn register_api_shadow(
        &mut self,
        plan: ApiMaintenanceRegistrationPlan<'_>,
    ) -> Result<Vec<AdjustmentShadowRegistrationReceipt>, Self::Error> {
        let parameters = maintenance_json_parameters(&plan.binds);
        query_json_rows(self, plan.sql, &parameters, "value", |value| {
            decode_registration_receipt(value)
        })
    }

    /// Execute one family append and decode its nested authenticated revision receipt.
    fn append_api_shadow(
        &mut self,
        plan: ApiMaintenancePredictionPlan<'_>,
    ) -> Result<Vec<AdjustmentShadowPredictionReceipt>, Self::Error> {
        let parameters = maintenance_json_parameters(&plan.binds);
        query_json_rows(self, plan.sql, &parameters, "value", |value| {
            decode_prediction_receipt(value)
        })
    }
}

impl WeatherApiMaintenancePool for NativePostgresPool {
    type MaintenanceSession<'pool>
        = NativePostgresSession
    where
        Self: 'pool;

    /// Checkout one independent lease from the retained native API pool.
    fn checkout_maintenance(&mut self) -> Result<Self::MaintenanceSession<'_>, Self::Error> {
        self.checkout()
    }

    /// Run all three schedule reads concurrently and retain the first completed failure.
    fn read_schedule_cohort(
        &mut self,
        plans: [AdjustmentShadowRegistrationSlotQueryPlan; 3],
    ) -> Result<[Vec<AdjustmentShadowRegistrationSlot>; 3], Self::Error> {
        let [temperature_plan, wind_plan, rain_plan] = plans;
        let temperature_pool = self.clone();
        let wind_pool = self.clone();
        let rain_pool = self.clone();
        let (results, first_error) = thread::scope(|scope| {
            let (sender, receiver) = mpsc::channel();
            let temperature_sender = sender.clone();
            let wind_sender = sender.clone();
            let rain_sender = sender.clone();
            scope.spawn(move || {
                let _ = temperature_sender
                    .send((0, execute_schedule_plan(temperature_pool, temperature_plan)));
            });
            scope.spawn(move || {
                let _ = wind_sender.send((1, execute_schedule_plan(wind_pool, wind_plan)));
            });
            scope.spawn(move || {
                let _ = rain_sender.send((2, execute_schedule_plan(rain_pool, rain_plan)));
            });
            drop(sender);

            let mut results: [Option<Vec<AdjustmentShadowRegistrationSlot>>; 3] =
                std::array::from_fn(|_| None);
            let mut first_error = None;
            // drain every completion while retaining the first reported failure
            for _ in 0..3 {
                match receiver.recv() {
                    Ok((index, Ok(rows))) => results[index] = Some(rows),
                    Ok((_, Err(error))) => {
                        // retain only the first completed database failure
                        if first_error.is_none() {
                            first_error = Some(error);
                        }
                    }
                    Err(_) => {
                        // represent a panicked or disconnected task only when no earlier error won
                        if first_error.is_none() {
                            first_error = Some(NativePostgresError::RuntimeUnavailable);
                        }
                        break;
                    }
                }
            }
            (results, first_error)
        });
        // return the first completed failure only after every scoped task has joined
        if let Some(error) = first_error {
            return Err(error);
        }
        let [temperature, wind, rain] = results.map(|rows| {
            rows.ok_or_else(|| {
                NativePostgresError::decode("schedule cohort", "query result is missing")
            })
        });
        Ok([temperature?, wind?, rain?])
    }
}

impl WeatherApiMaintenanceWritePool for NativePostgresPool {
    type MaintenanceWriteSession<'pool>
        = NativePostgresSession
    where
        Self: 'pool;

    /// Checkout one independent compact-write lease from the retained native pool.
    fn checkout_maintenance_write(
        &mut self,
    ) -> Result<Self::MaintenanceWriteSession<'_>, Self::Error> {
        self.checkout()
    }
}

/// Execute one schedule plan with the API pool's existing release classification.
fn execute_schedule_plan(
    pool: NativePostgresPool,
    plan: AdjustmentShadowRegistrationSlotQueryPlan,
) -> Result<Vec<AdjustmentShadowRegistrationSlot>, NativePostgresError> {
    let session = pool.checkout()?;
    run_api_pool_query(
        session,
        |session| session.read_schedule_slot(plan),
        <NativePostgresPool as WeatherApiReadPool>::classify_session_failure,
    )
}

/// Execute one JSON-returning facade and decode every native result row.
fn query_json_rows<T>(
    session: &mut NativePostgresSession,
    sql: &str,
    parameters: &[Parameter],
    column: &str,
    decode: impl Fn(&JsonValue) -> Result<T, NativePostgresError>,
) -> Result<Vec<T>, NativePostgresError> {
    session
        .query(sql, parameters)?
        .iter()
        .map(|row| {
            let value = field::json(row, column)?.ok_or_else(|| {
                NativePostgresError::decode(column, "database JSON result is null")
            })?;
            decode(&value)
        })
        .collect()
}

/// Map one publication facade's closed bind sequence to native PostgreSQL types.
fn publication_parameters<const N: usize>(
    binds: &[MaintenancePublicationBind<'_>; N],
) -> Result<Vec<Parameter>, NativePostgresError> {
    binds
        .iter()
        .map(|bind| match bind {
            MaintenancePublicationBind::Text(value) => Ok(Parameter::Text((*value).to_owned())),
            MaintenancePublicationBind::Integer(value) => Ok(Parameter::I32(*value)),
            MaintenancePublicationBind::RevisionKeyJson(key) => {
                json_parameter(revision_key_value(key)?)
            }
            MaintenancePublicationBind::GapJson(gap) => json_parameter(JsonValue::Object(vec![
                member("logicalKeySha256", string(&gap.logicalKeySha256)),
                member(
                    "projectionIdentitySha256",
                    nullable_string(&gap.projectionIdentitySha256),
                ),
                member(
                    "projectionKind",
                    string(&wire_value(
                        gap.projectionKind,
                        AdjustmentRevisionProjectionKind::WIRE_VALUES,
                        "revision projection kind",
                    )?),
                ),
                member("projectionSha256", nullable_string(&gap.projectionSha256)),
                member(
                    "reason",
                    string(&wire_value(
                        gap.reason,
                        AdjustmentRevisionGapReason::WIRE_VALUES,
                        "revision gap reason",
                    )?),
                ),
            ])),
        })
        .collect()
}

/// Map one already-canonical maintenance JSON bind without re-encoding it.
fn maintenance_json_parameters(binds: &[ApiMaintenanceJsonBind<'_>; 1]) -> Vec<Parameter> {
    binds
        .iter()
        .map(|bind| match bind {
            ApiMaintenanceJsonBind::CanonicalJson(value) => Parameter::Json((*value).to_owned()),
        })
        .collect()
}

/// Encode one generated JSON parameter through the approved native JSON owner.
fn json_parameter(value: JsonValue) -> Result<Parameter, NativePostgresError> {
    let mut bytes = NativeJson::encode_ordered(&value)
        .ok_or_else(|| NativePostgresError::decode("JSON parameter", "encoding failed"))?;
    // remove only the archive serializer's framing newline for a compact database bind
    if bytes.pop() != Some(b'\n') {
        return Err(NativePostgresError::decode(
            "JSON parameter",
            "encoded JSON is not newline framed",
        ));
    }
    String::from_utf8(bytes)
        .map(Parameter::Json)
        .map_err(|error| NativePostgresError::decode("JSON parameter", error))
}

/// Project one closed admission key without adding an enum variant tag.
fn revision_key_value(
    key: &AdjustmentRevisionAdmissionKey,
) -> Result<JsonValue, NativePostgresError> {
    // retain each database facade's exact key object
    let value = match key {
        AdjustmentRevisionAdmissionKey::WeatherRecord(key) => JsonValue::Object(vec![
            member("productRunAt", nullable_string(&key.productRunAt)),
            member("sourceId", string(&key.sourceId)),
            member(
                "sourceKind",
                string(&wire_value(
                    key.sourceKind,
                    AdjustmentWeatherRevisionPointerSourceKindEnum::WIRE_VALUES,
                    "weather revision source kind",
                )?),
            ),
            member("storedContentSha256", string(&key.storedContentSha256)),
            member("validAt", string(&key.validAt)),
        ]),
        AdjustmentRevisionAdmissionKey::ForecastAnchor(key) => {
            if key.leadHours > MAXIMUM_SAFE_INTEGER {
                return Err(NativePostgresError::decode(
                    "leadHours",
                    "integer exceeds the generated safe range",
                ));
            }
            JsonValue::Object(vec![
                member("leadHours", JsonValue::Number(key.leadHours as f64)),
                member("sourceId", string(&key.sourceId)),
                member("storedContentSha256", string(&key.storedContentSha256)),
                member("validAt", string(&key.validAt)),
            ])
        }
        AdjustmentRevisionAdmissionKey::RainGate(key) => JsonValue::Object(vec![
            member("inputSha256", string(&key.inputSha256)),
            member("modelSha256", string(&key.modelSha256)),
            member("runInitializedAt", string(&key.runInitializedAt)),
            member("storedContentSha256", string(&key.storedContentSha256)),
        ]),
        AdjustmentRevisionAdmissionKey::EcmwfTemperature(key) => JsonValue::Object(vec![
            member(
                "providerResponseSha256",
                string(&key.providerResponseSha256),
            ),
            member("runInitializedAt", string(&key.runInitializedAt)),
            member("siteId", string(&key.siteId)),
            member("storedContentSha256", string(&key.storedContentSha256)),
        ]),
    };
    Ok(value)
}

/// Decode one exact bounded registration-slot result and bind it to the requested family.
fn decode_registration_slot(
    value: &JsonValue,
    requested_family: AdjustmentShadowFamily,
) -> Result<AdjustmentShadowRegistrationSlot, NativePostgresError> {
    const KEYS: &[&str] = &[
        "contractVersion",
        "epochWitnessSha256",
        "family",
        "horizonEndAt",
        "registrationSha256",
        "scheduleContractSha256",
        "state",
        "terminalAt",
    ];
    exact_object(value, KEYS, "registration slot")?;
    if encoded_length(value)? > MAXIMUM_REGISTRATION_SLOT_BYTES {
        return Err(NativePostgresError::decode(
            "registration slot",
            "JSON exceeds the receipt bound",
        ));
    }
    let contract_version = text(value, "contractVersion")?;
    if contract_version != AdjustmentShadowRegistrationSlotContractVersionLiteral::WIRE_VALUE {
        return Err(NativePostgresError::decode(
            "contractVersion",
            "unexpected registration slot version",
        ));
    }
    let family = enum_field(value, "family", AdjustmentShadowFamily::WIRE_VALUES)?;
    if family != requested_family {
        return Err(NativePostgresError::decode(
            "family",
            "registration slot family differs from query",
        ));
    }
    let state = enum_field(
        value,
        "state",
        AdjustmentShadowRegistrationSlotStateEnum::WIRE_VALUES,
    )?;
    let registration_sha256 = nullable_text(value, "registrationSha256")?;
    let terminal_at = nullable_text(value, "terminalAt")?;
    // bind free and occupied slot evidence exactly as the source validator
    match state {
        AdjustmentShadowRegistrationSlotStateEnum::Free => {
            if registration_sha256 != Nullable::Null || terminal_at != Nullable::Null {
                return Err(NativePostgresError::decode(
                    "registration slot",
                    "free slot carries occupied evidence",
                ));
            }
        }
        AdjustmentShadowRegistrationSlotStateEnum::BusyV2Legacy
        | AdjustmentShadowRegistrationSlotStateEnum::BusyV3 => {
            let Nullable::Value(registration) = &registration_sha256 else {
                return Err(NativePostgresError::decode(
                    "registrationSha256",
                    "occupied slot has no registration",
                ));
            };
            require_hash(registration, "registrationSha256")?;
            let Nullable::Value(terminal) = &terminal_at else {
                return Err(NativePostgresError::decode(
                    "terminalAt",
                    "occupied slot has no terminal clock",
                ));
            };
            require_utc(terminal, "terminalAt")?;
        }
    }
    let epoch_witness = text(value, "epochWitnessSha256")?;
    let schedule_contract = text(value, "scheduleContractSha256")?;
    let horizon_end = text(value, "horizonEndAt")?;
    require_hash(&epoch_witness, "epochWitnessSha256")?;
    require_hash(&schedule_contract, "scheduleContractSha256")?;
    require_utc(&horizon_end, "horizonEndAt")?;
    Ok(AdjustmentShadowRegistrationSlot {
        contractVersion: AdjustmentShadowRegistrationSlotContractVersionLiteral,
        epochWitnessSha256: epoch_witness,
        family,
        horizonEndAt: horizon_end,
        registrationSha256: registration_sha256,
        scheduleContractSha256: schedule_contract,
        state,
        terminalAt: terminal_at,
    })
}

/// Decode one exact compact registration receipt.
fn decode_registration_receipt(
    value: &JsonValue,
) -> Result<AdjustmentShadowRegistrationReceipt, NativePostgresError> {
    exact_object(
        value,
        &["inserted", "registrationSha256"],
        "registration receipt",
    )?;
    let registration = text(value, "registrationSha256")?;
    require_hash(&registration, "registrationSha256")?;
    Ok(AdjustmentShadowRegistrationReceipt {
        inserted: boolean(value, "inserted")?,
        registrationSha256: registration,
    })
}

/// Decode one exact prediction receipt and authenticate its nested revision receipt.
fn decode_prediction_receipt(
    value: &JsonValue,
) -> Result<AdjustmentShadowPredictionReceipt, NativePostgresError> {
    exact_object(
        value,
        &[
            "committedAt",
            "inserted",
            "predictionSha256",
            "revisionReceipt",
        ],
        "prediction receipt",
    )?;
    let committed_at = text(value, "committedAt")?;
    let prediction_sha256 = text(value, "predictionSha256")?;
    require_utc(&committed_at, "committedAt")?;
    require_hash(&prediction_sha256, "predictionSha256")?;
    Ok(AdjustmentShadowPredictionReceipt {
        committedAt: committed_at,
        inserted: boolean(value, "inserted")?,
        predictionSha256: prediction_sha256,
        revisionReceipt: decode_commit_receipt(member_value(value, "revisionReceipt")?)?,
    })
}

/// Decode and authenticate one database revision commit receipt.
fn decode_commit_receipt(
    value: &JsonValue,
) -> Result<AdjustmentRevisionCommitReceipt, NativePostgresError> {
    ArchiveWireCodec::new(NativeWirePrimitives)
        .decode_revision_commit_value(value)
        .ok_or_else(|| NativePostgresError::decode("revision receipt", "contract mismatch"))
}

/// Decode one exact permanent revision-gap marker.
fn decode_revision_gap_marker(
    value: &JsonValue,
) -> Result<AdjustmentRevisionGapMarker, NativePostgresError> {
    const KEYS: &[&str] = &[
        "contractVersion",
        "logicalKeySha256",
        "projectionIdentitySha256",
        "projectionKind",
        "projectionSha256",
        "reason",
        "storedContentSha256",
    ];
    exact_object(value, KEYS, "revision gap marker")?;
    if text(value, "contractVersion")? != "adjustment-revision-gap-marker/v1" {
        return Err(NativePostgresError::decode(
            "contractVersion",
            "unexpected revision gap marker version",
        ));
    }
    let logical_key = text(value, "logicalKeySha256")?;
    let stored_content = text(value, "storedContentSha256")?;
    let projection_identity = nullable_text(value, "projectionIdentitySha256")?;
    let projection_sha = nullable_text(value, "projectionSha256")?;
    require_hash(&logical_key, "logicalKeySha256")?;
    require_hash(&stored_content, "storedContentSha256")?;
    // preserve the database marker's paired nullable projection identities
    match (&projection_identity, &projection_sha) {
        (Nullable::Null, Nullable::Null) => {}
        (Nullable::Value(identity), Nullable::Value(projection)) if identity == projection => {
            require_hash(identity, "projectionIdentitySha256")?;
        }
        _ => {
            return Err(NativePostgresError::decode(
                "revision gap marker",
                "projection identities differ",
            ));
        }
    }
    Ok(AdjustmentRevisionGapMarker {
        logical_key_sha256: logical_key,
        projection_identity_sha256: projection_identity,
        projection_kind: enum_field(
            value,
            "projectionKind",
            AdjustmentRevisionProjectionKind::WIRE_VALUES,
        )?,
        projection_sha256: projection_sha,
        reason: enum_field(value, "reason", AdjustmentRevisionGapReason::WIRE_VALUES)?,
        stored_content_sha256: stored_content,
    })
}

/// Require one exact object key set without accepting duplicates or extensions.
fn exact_object<'a>(
    value: &'a JsonValue,
    keys: &[&str],
    label: &str,
) -> Result<&'a [(Vec<u16>, JsonValue)], NativePostgresError> {
    let JsonValue::Object(fields) = value else {
        return Err(NativePostgresError::decode(label, "expected JSON object"));
    };
    if fields.len() != keys.len()
        || !keys.iter().all(|key| {
            fields
                .iter()
                .filter(|(name, _)| name.iter().copied().eq(key.encode_utf16()))
                .count()
                == 1
        })
    {
        return Err(NativePostgresError::decode(label, "unexpected JSON keys"));
    }
    Ok(fields)
}

/// Borrow one exact object member without normalizing its UTF-16 key.
fn member_value<'a>(
    value: &'a JsonValue,
    name: &str,
) -> Result<&'a JsonValue, NativePostgresError> {
    let fields = exact_object_any(value, name)?;
    fields
        .iter()
        .find_map(|(key, value)| key.iter().copied().eq(name.encode_utf16()).then_some(value))
        .ok_or_else(|| NativePostgresError::decode(name, "missing JSON field"))
}

/// Require an object before reading one already-validated member.
fn exact_object_any<'a>(
    value: &'a JsonValue,
    label: &str,
) -> Result<&'a [(Vec<u16>, JsonValue)], NativePostgresError> {
    let JsonValue::Object(fields) = value else {
        return Err(NativePostgresError::decode(label, "expected JSON object"));
    };
    Ok(fields)
}

/// Decode one required JSON string without scalar coercion.
fn text(value: &JsonValue, name: &str) -> Result<String, NativePostgresError> {
    let JsonValue::String(units) = member_value(value, name)? else {
        return Err(NativePostgresError::decode(name, "expected JSON string"));
    };
    String::from_utf16(units).map_err(|error| NativePostgresError::decode(name, error))
}

/// Decode one required JSON boolean without scalar coercion.
fn boolean(value: &JsonValue, name: &str) -> Result<bool, NativePostgresError> {
    let JsonValue::Boolean(value) = member_value(value, name)? else {
        return Err(NativePostgresError::decode(name, "expected JSON boolean"));
    };
    Ok(*value)
}

/// Decode one nullable JSON string into generated nullable storage.
fn nullable_text(value: &JsonValue, name: &str) -> Result<Nullable<String>, NativePostgresError> {
    // preserve JSON null independently from an empty string
    match member_value(value, name)? {
        JsonValue::Null => Ok(Nullable::Null),
        JsonValue::String(units) => String::from_utf16(units)
            .map(Nullable::Value)
            .map_err(|error| NativePostgresError::decode(name, error)),
        _ => Err(NativePostgresError::decode(
            name,
            "expected JSON string or null",
        )),
    }
}

/// Decode one generated enum through its exact wire values.
fn enum_field<T: Copy + PartialEq>(
    value: &JsonValue,
    name: &str,
    values: &[(&str, T)],
) -> Result<T, NativePostgresError> {
    let wire = text(value, name)?;
    values
        .iter()
        .find_map(|(candidate, value)| (*candidate == wire).then_some(*value))
        .ok_or_else(|| NativePostgresError::decode(name, "unsupported enum value"))
}

/// Resolve one typed enum to its exact database wire spelling.
fn wire_value<T: Copy + PartialEq>(
    value: T,
    values: &[(&str, T)],
    label: &str,
) -> Result<String, NativePostgresError> {
    values
        .iter()
        .find_map(|(wire, candidate)| (*candidate == value).then_some((*wire).to_owned()))
        .ok_or_else(|| NativePostgresError::decode(label, "unsupported enum value"))
}

/// Require one exact lowercase SHA-256 wire value.
fn require_hash(value: &str, name: &str) -> Result<(), NativePostgresError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(NativePostgresError::decode(name, "invalid SHA-256"));
    }
    Ok(())
}

/// Require one exact canonical UTC millisecond instant.
fn require_utc(value: &str, name: &str) -> Result<(), NativePostgresError> {
    NativeWirePrimitives
        .canonical_utc_milliseconds(value)
        .map(|_| ())
        .ok_or_else(|| NativePostgresError::decode(name, "invalid canonical UTC instant"))
}

/// Measure one decoded JSON document through the approved compact encoder.
fn encoded_length(value: &JsonValue) -> Result<usize, NativePostgresError> {
    NativeJson::encode_ordered(value)
        .map(|bytes| bytes.len().saturating_sub(1))
        .ok_or_else(|| NativePostgresError::decode("JSON value", "encoding failed"))
}

/// Build one generated JSON object member.
fn member(name: &str, value: JsonValue) -> (Vec<u16>, JsonValue) {
    (name.encode_utf16().collect(), value)
}

/// Build one generated JSON string value.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Build one generated nullable JSON string value.
fn nullable_string(value: &Nullable<String>) -> JsonValue {
    // preserve generated null independently from a string value
    match value {
        Nullable::Null => JsonValue::Null,
        Nullable::Value(value) => string(value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Decode the exact free schedule-slot contract and reject occupied evidence on a free slot.
    #[test]
    fn schedule_slot_decoder_binds_family_and_occupancy() {
        let value = JsonValue::Object(vec![
            member(
                "contractVersion",
                string("adjustment-shadow-registration-slot/v3"),
            ),
            member("epochWitnessSha256", string(&"a".repeat(64))),
            member("family", string("temperature")),
            member("horizonEndAt", string("2026-11-24T20:00:00.000Z")),
            member("registrationSha256", JsonValue::Null),
            member("scheduleContractSha256", string(&"b".repeat(64))),
            member("state", string("free")),
            member("terminalAt", JsonValue::Null),
        ]);
        let slot = decode_registration_slot(&value, AdjustmentShadowFamily::Temperature).unwrap();
        assert_eq!(slot.state, AdjustmentShadowRegistrationSlotStateEnum::Free);
        assert!(decode_registration_slot(&value, AdjustmentShadowFamily::Wind).is_err());
    }

    /// Preserve all four admission-key shapes without adding Rust enum tags.
    #[test]
    fn revision_key_encoding_uses_closed_database_objects() {
        let key = AdjustmentRevisionAdmissionKey::RainGate(
            weather_contracts::AdjustmentRainGateRevisionAdmissionKey {
                inputSha256: "a".repeat(64),
                modelSha256: "b".repeat(64),
                runInitializedAt: "2026-11-24T20:00:00.000Z".to_owned(),
                storedContentSha256: "c".repeat(64),
            },
        );
        let value = revision_key_value(&key).unwrap();
        assert!(
            exact_object(
                &value,
                &[
                    "inputSha256",
                    "modelSha256",
                    "runInitializedAt",
                    "storedContentSha256",
                ],
                "rain key",
            )
            .is_ok()
        );
    }

    /// Reject JSON extension fields and noncanonical hash spellings at the decode boundary.
    #[test]
    fn registration_receipt_decoder_is_exact() {
        let valid = JsonValue::Object(vec![
            member("inserted", JsonValue::Boolean(true)),
            member("registrationSha256", string(&"d".repeat(64))),
        ]);
        assert!(decode_registration_receipt(&valid).is_ok());
        let extended = JsonValue::Object(vec![
            member("inserted", JsonValue::Boolean(true)),
            member("registrationSha256", string(&"d".repeat(64))),
            member("extra", JsonValue::Null),
        ]);
        assert!(decode_registration_receipt(&extended).is_err());
    }

    /// Retain safe numeric JSON values used by the closed forecast-anchor key.
    #[test]
    fn safe_integer_boundary_matches_generated_contract() {
        assert_eq!(MAXIMUM_SAFE_INTEGER as f64, 9_007_199_254_740_991.0);
    }
}
