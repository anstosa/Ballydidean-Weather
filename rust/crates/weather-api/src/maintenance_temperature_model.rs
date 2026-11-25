//! Evaluate private temperature candidates and incumbents on retained source rows.

use weather_adjustment::BorrowedTemperatureMosServingModel;
use weather_contracts::{
    ApplyTemperatureCanaryInputV1, EcmwfTemperatureCanaryRecentErrorState,
    ForecastAdjustmentTemperatureMaintenanceRuntimePackage,
    ForecastAdjustmentTemperatureMaintenanceRuntimePackageSourceAdapterVersionLiteral,
    ForecastAdjustmentTemperatureMaintenanceRuntimePackageSourceMaximumReceiptAgeHoursLiteral,
    ForecastAdjustmentTemperatureMaintenanceRuntimePackageSourceSourceDelayHoursLiteral,
    ForecastTemperatureCanaryDecisionV1, ForecastTemperatureCanaryDecisionV1StateEnum, Nullable,
    TemperatureCanaryDecisionReasonValue, TemperatureCanarySourceForecast,
    TemperatureCanarySourceForecastDatasetLiteral, TemperatureCanarySourceForecastModelCycleEnum,
    TemperatureCanarySourceForecastProviderKeyLiteral,
    TemperatureCanarySourceForecastUpstreamModelLiteral, TemperatureMaintenanceCandidateRow,
    TemperatureMaintenanceCandidateRowFallbackCodeEnum, TemperatureMaintenanceIncumbentRow,
    TemperatureMosRuntimeRecentErrorState, TemperatureMosRuntimeRecentErrorStateCohortLiteral,
};

use crate::maintenance_source_projection::{
    MaintenanceSourceField, MaintenanceSourceProjection, MaintenanceSourceProjectionRows,
    MaintenanceSourceValue, is_source_digest,
};
use crate::temperature_model::AuthorityFreeTemperatureMosOwner;
use crate::{
    AdmittedTemperatureCanaryRuntime, AdmittedTemperatureMosModelOwner, CurrentTimeResolver,
    ForecastTemperatureAdjustmentBoundary, ForecastTemperatureMosAdmissionBoundary,
};

const TEMPERATURE_SOURCE_ROWS: usize = 12;

/// Bound failures while decoding or evaluating one admitted private source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemperatureMaintenanceModelError {
    /// The closed retained source could not reconstruct the generated input.
    Source,
    /// The typed candidate or incumbent owner could not evaluate the input.
    Candidate,
}

/// The actual startup incumbent, including a retained disabled reason.
#[derive(Clone, Copy)]
pub enum TemperatureMaintenanceIncumbent<'a> {
    /// Reuse the actual admitted model, authorization, and source owner.
    Active {
        runtime: &'a AdmittedTemperatureCanaryRuntime,
        owner: &'a AdmittedTemperatureMosModelOwner<'a, BorrowedTemperatureMosServingModel<'a>>,
    },
    /// Preserve the generated startup reason when no incumbent bundle is active.
    Disabled {
        reason_code: TemperatureCanaryDecisionReasonValue,
    },
}

/// Evaluate one verified authority-free permanent temperature package.
pub fn evaluate_temperature_maintenance_candidate(
    package: &ForecastAdjustmentTemperatureMaintenanceRuntimePackage,
    source: &MaintenanceSourceProjection<'_>,
    time: &dyn CurrentTimeResolver,
    admission: &dyn ForecastTemperatureMosAdmissionBoundary,
) -> Result<Vec<TemperatureMaintenanceCandidateRow>, TemperatureMaintenanceModelError> {
    // a typed package still requires its variable bundle content address
    if !is_source_digest(&package.bundleSha256) {
        return Err(TemperatureMaintenanceModelError::Candidate);
    }
    let owner = AuthorityFreeTemperatureMosOwner::new(
        &package.bundleSha256,
        BorrowedTemperatureMosServingModel::Permanent(&package.model),
        ForecastAdjustmentTemperatureMaintenanceRuntimePackageSourceAdapterVersionLiteral::WIRE_VALUE,
        ForecastAdjustmentTemperatureMaintenanceRuntimePackageSourceMaximumReceiptAgeHoursLiteral::WIRE_VALUE,
        ForecastAdjustmentTemperatureMaintenanceRuntimePackageSourceSourceDelayHoursLiteral::WIRE_VALUE,
        time,
        admission,
    );
    let inputs = decode_temperature_inputs(source)?;
    let mut rows = Vec::with_capacity(inputs.len());
    // preserve validated source order while withholding activation authority
    for decoded in inputs {
        let decision = owner.apply(&decoded.input, None);
        let would_apply = decision.state == ForecastTemperatureCanaryDecisionV1StateEnum::Active
            && matches!(&decision.correctedTemperatureC, Nullable::Value(_));
        let candidate_temperature = selected_temperature(&decision, &decoded);
        let fallback_code = if would_apply {
            TemperatureMaintenanceCandidateRowFallbackCodeEnum::None
        } else {
            candidate_fallback_code(&decision.reasonCode)
        };
        rows.push(TemperatureMaintenanceCandidateRow {
            candidateTemperatureC: candidate_temperature,
            fallbackCode: fallback_code,
            validAt: decoded.input.validAt,
            wouldApply: would_apply,
        });
    }
    Ok(rows)
}

/// Replay the admitted startup incumbent or its exact disabled reason.
pub fn evaluate_temperature_maintenance_incumbent(
    incumbent: TemperatureMaintenanceIncumbent<'_>,
    source: &MaintenanceSourceProjection<'_>,
) -> Result<Vec<TemperatureMaintenanceIncumbentRow>, TemperatureMaintenanceModelError> {
    let inputs = decode_temperature_inputs(source)?;
    let mut rows = Vec::with_capacity(inputs.len());
    // apply one unchanged startup disposition to every captured source hour
    for decoded in inputs {
        let (applied, temperature, reason_code) = match incumbent {
            TemperatureMaintenanceIncumbent::Active { runtime, owner } => {
                let decision = owner
                    .apply(runtime, &decoded.input, None)
                    .map_err(|_| TemperatureMaintenanceModelError::Candidate)?;
                let applied = decision.state
                    == ForecastTemperatureCanaryDecisionV1StateEnum::Active
                    && matches!(&decision.correctedTemperatureC, Nullable::Value(_));
                (
                    applied,
                    selected_temperature(&decision, &decoded),
                    decision_reason_text(&decision.reasonCode),
                )
            }
            TemperatureMaintenanceIncumbent::Disabled { reason_code } => (
                false,
                raw_fallback_temperature(&decoded),
                Nullable::Value(reason_wire_value(reason_code).to_owned()),
            ),
        };
        rows.push(TemperatureMaintenanceIncumbentRow {
            applied,
            incumbentTemperatureC: temperature,
            reasonCode: reason_code,
            validAt: decoded.input.validAt,
        });
    }
    Ok(rows)
}

struct DecodedTemperatureInput {
    input: ApplyTemperatureCanaryInputV1,
    raw_temperature_c: f64,
}

/// Decode the closed temperature rows back into the generated MOS application input.
fn decode_temperature_inputs(
    source: &MaintenanceSourceProjection<'_>,
) -> Result<Vec<DecodedTemperatureInput>, TemperatureMaintenanceModelError> {
    let MaintenanceSourceProjectionRows::Temperature {
        recent_error_state,
        rows,
    } = source.rows()
    else {
        return Err(TemperatureMaintenanceModelError::Source);
    };
    // refuse partial or extended private sources before numerical evaluation
    if rows.len() != TEMPERATURE_SOURCE_ROWS {
        return Err(TemperatureMaintenanceModelError::Source);
    }
    let recent_error_state = project_recent_error_state(recent_error_state);
    let evaluated_at = source.header().issued_at.utc.clone();
    let mut inputs = Vec::with_capacity(rows.len());
    // decode every value only after checking its exact schema position and key
    for (index, row) in rows.iter().enumerate() {
        let valid_at = source_text(row, 0, "validAt")?.to_owned();
        let lead_hours = source_number(row, 1, "leadHours")?;
        // retain exact one-based row lead geometry from the validated projection
        if lead_hours != (index + 1) as u64 {
            return Err(TemperatureMaintenanceModelError::Source);
        }
        let model_lead_hours = source_number(row, 2, "modelLeadHours")?;
        let run_initialized_at = source_text(row, 3, "referenceAt")?.to_owned();
        let first_received_at = source_text(row, 4, "receivedAt")?.to_owned();
        source_text(row, 5, "sourceSha256")?;
        source_text(row, 6, "contentSha256")?;
        let adapter_version = source_text(row, 7, "adapterVersion")?.to_owned();
        // fixed typed identities must remain unchanged after source serialization
        if source_text(row, 8, "dataset")? != "single_run"
            || source_text(row, 9, "providerKey")? != "open-meteo"
            || source_text(row, 12, "upstreamModel")? != "ecmwf_ifs"
        {
            return Err(TemperatureMaintenanceModelError::Source);
        }
        let provider_response_sha256 = source_text(row, 10, "providerResponseSha256")?.to_owned();
        let model_cycle = decode_model_cycle(source_text(row, 11, "modelCycle")?)?;
        let raw_temperature_c = required_binary64(row, 13, "rawTemperatureC64")?;
        let raw_relative_humidity_percent =
            nullable_binary64(row, 14, "rawRelativeHumidityPercent64")?;
        let raw_wind_speed_mps = nullable_binary64(row, 15, "rawWindSpeedMps64")?;
        source_text(row, 16, "bestMatchContentSha256")?;
        source_text(row, 17, "bestMatchProductRunAt")?;
        source_text(row, 18, "bestMatchSourceId")?;
        let raw_best_match_temperature_c = nullable_binary64(row, 19, "bestMatchTemperatureC64")?;
        inputs.push(DecodedTemperatureInput {
            input: ApplyTemperatureCanaryInputV1 {
                evaluatedAt: evaluated_at.clone(),
                rawBestMatchTemperatureC: raw_best_match_temperature_c,
                recentErrorState: Nullable::Value(recent_error_state.clone()),
                sourceForecast: Nullable::Value(TemperatureCanarySourceForecast {
                    adapterVersion: adapter_version,
                    dataset: TemperatureCanarySourceForecastDatasetLiteral,
                    firstReceivedAt: first_received_at,
                    modelCycle: model_cycle,
                    modelLeadHours: model_lead_hours,
                    providerKey: TemperatureCanarySourceForecastProviderKeyLiteral,
                    providerResponseSha256: provider_response_sha256,
                    rawRelativeHumidityPercent: raw_relative_humidity_percent,
                    rawTemperatureC: raw_temperature_c,
                    rawWindSpeedMps: raw_wind_speed_mps,
                    runInitializedAt: run_initialized_at,
                    upstreamModel: TemperatureCanarySourceForecastUpstreamModelLiteral,
                    validAt: valid_at.clone(),
                }),
                validAt: valid_at,
            },
            raw_temperature_c,
        });
    }
    Ok(inputs)
}

/// Borrow one text field only at its exact frozen schema position.
fn source_text<'a>(
    row: &'a [MaintenanceSourceField; 20],
    index: usize,
    key: &'static str,
) -> Result<&'a str, TemperatureMaintenanceModelError> {
    // close both key order and scalar representation before returning text
    match row.get(index) {
        Some((actual, MaintenanceSourceValue::Text(value))) if *actual == key => Ok(value),
        _ => Err(TemperatureMaintenanceModelError::Source),
    }
}

/// Read one unsigned field only at its exact frozen schema position.
fn source_number(
    row: &[MaintenanceSourceField; 20],
    index: usize,
    key: &'static str,
) -> Result<u64, TemperatureMaintenanceModelError> {
    // reject text aliases for numeric source fields
    match row.get(index) {
        Some((actual, MaintenanceSourceValue::Number(value))) if *actual == key => Ok(*value),
        _ => Err(TemperatureMaintenanceModelError::Source),
    }
}

/// Decode one required canonical binary64 source field.
fn required_binary64(
    row: &[MaintenanceSourceField; 20],
    index: usize,
    key: &'static str,
) -> Result<f64, TemperatureMaintenanceModelError> {
    // required raw temperature cannot use the nullable source representation
    match row.get(index) {
        Some((actual, MaintenanceSourceValue::Text(value))) if *actual == key => {
            decode_binary64(value)
        }
        _ => Err(TemperatureMaintenanceModelError::Source),
    }
}

/// Decode one nullable canonical binary64 source field.
fn nullable_binary64(
    row: &[MaintenanceSourceField; 20],
    index: usize,
    key: &'static str,
) -> Result<Nullable<f64>, TemperatureMaintenanceModelError> {
    // retain null separately from every exact finite binary64 bit pattern
    match row.get(index) {
        Some((actual, MaintenanceSourceValue::Null)) if *actual == key => Ok(Nullable::Null),
        Some((actual, MaintenanceSourceValue::Text(value))) if *actual == key => {
            decode_binary64(value).map(Nullable::Value)
        }
        _ => Err(TemperatureMaintenanceModelError::Source),
    }
}

/// Decode the canonical sixteen-digit lowercase binary64 representation.
fn decode_binary64(value: &str) -> Result<f64, TemperatureMaintenanceModelError> {
    // reject aliases before parsing the exact source bits
    if value.len() != 16
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err(TemperatureMaintenanceModelError::Source);
    }
    let bits =
        u64::from_str_radix(value, 16).map_err(|_| TemperatureMaintenanceModelError::Source)?;
    let number = f64::from_bits(bits);
    // validated source projections contain only finite physical values
    if !number.is_finite() {
        return Err(TemperatureMaintenanceModelError::Source);
    }
    Ok(number)
}

/// Decode the closed ECMWF model-cycle source identity.
fn decode_model_cycle(
    value: &str,
) -> Result<TemperatureCanarySourceForecastModelCycleEnum, TemperatureMaintenanceModelError> {
    // reject aliases outside the generated two-era cycle vocabulary
    match value {
        "49r1" => Ok(TemperatureCanarySourceForecastModelCycleEnum::Value49r1),
        "50r1" => Ok(TemperatureCanarySourceForecastModelCycleEnum::Value50r1),
        _ => Err(TemperatureMaintenanceModelError::Source),
    }
}

/// Copy the retained recent-error state into the generated numerical input.
fn project_recent_error_state(
    state: &EcmwfTemperatureCanaryRecentErrorState,
) -> TemperatureMosRuntimeRecentErrorState {
    TemperatureMosRuntimeRecentErrorState {
        b24C: state.b24C.clone(),
        b72C: state.b72C.clone(),
        cohort: TemperatureMosRuntimeRecentErrorStateCohortLiteral,
        localDates: state.localDates,
        mad72C: state.mad72C.clone(),
        maximumSourceRunInitializedAt: state.maximumSourceRunInitializedAt.clone(),
        maximumSourceValidAt: state.maximumSourceValidAt.clone(),
        n24: state.n24,
        n72: state.n72,
        sourceKeys: state.sourceKeys.clone(),
        supported: state.supported,
        targetRunInitializedAt: state.targetRunInitializedAt.clone(),
        windowEndValidAt: state.windowEndValidAt.clone(),
    }
}

/// Select the corrected value only for an active decision.
fn selected_temperature(
    decision: &ForecastTemperatureCanaryDecisionV1,
    decoded: &DecodedTemperatureInput,
) -> f64 {
    // preserve raw fallback whenever the decision did not produce a correction
    match (&decision.state, &decision.correctedTemperatureC) {
        (ForecastTemperatureCanaryDecisionV1StateEnum::Active, Nullable::Value(value)) => *value,
        _ => raw_fallback_temperature(decoded),
    }
}

/// Prefer the Best Match comparator and then the retained raw source temperature.
fn raw_fallback_temperature(decoded: &DecodedTemperatureInput) -> f64 {
    // mirror JavaScript's nullish fallback without treating zero as absent
    match &decoded.input.rawBestMatchTemperatureC {
        Nullable::Value(value) => *value,
        Nullable::Null => decoded.raw_temperature_c,
    }
}

/// Map the bounded generated reason vocabulary to the frozen evidence code.
fn candidate_fallback_code(
    reason: &Nullable<TemperatureCanaryDecisionReasonValue>,
) -> TemperatureMaintenanceCandidateRowFallbackCodeEnum {
    // retain missing-source and physical-domain refusals as distinct evidence
    match reason {
        Nullable::Value(
            TemperatureCanaryDecisionReasonValue::MissingSourceForecast
            | TemperatureCanaryDecisionReasonValue::SourceNotAvailable
            | TemperatureCanaryDecisionReasonValue::SourceStale
            | TemperatureCanaryDecisionReasonValue::SourceTimeMismatch,
        ) => TemperatureMaintenanceCandidateRowFallbackCodeEnum::MissingSource,
        Nullable::Value(TemperatureCanaryDecisionReasonValue::OutsideOperationalWindow) => {
            TemperatureMaintenanceCandidateRowFallbackCodeEnum::PhysicalCap
        }
        _ => TemperatureMaintenanceCandidateRowFallbackCodeEnum::Ineligible,
    }
}

/// Convert one generated optional reason to its exact wire string.
fn decision_reason_text(
    reason: &Nullable<TemperatureCanaryDecisionReasonValue>,
) -> Nullable<String> {
    // active decisions retain null while every fallback retains its generated label
    match reason {
        Nullable::Null => Nullable::Null,
        Nullable::Value(reason) => Nullable::Value(reason_wire_value(*reason).to_owned()),
    }
}

/// Resolve one generated reason through its authoritative wire catalog.
fn reason_wire_value(reason: TemperatureCanaryDecisionReasonValue) -> &'static str {
    TemperatureCanaryDecisionReasonValue::WIRE_VALUES
        .iter()
        .find_map(|(wire, value)| (*value == reason).then_some(*wire))
        .expect("generated temperature reason must have a wire value")
}
