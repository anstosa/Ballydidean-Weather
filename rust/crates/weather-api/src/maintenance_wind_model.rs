//! Replay the installed wind candidate and the startup incumbent on one checked source.

use weather_adjustment::{
    AdmittedWindBundleRow, AdmittedWindRowCalendar, AdmittedWindRowFeatures,
    AdmittedWindRowSourceIdentity, AdmittedWindV2MetricOrder, WindV2RowOutcome,
    evaluate_admitted_wind_bundle_row, evaluate_admitted_wind_v2_row,
};
use weather_contracts::{
    CanonicalWeatherMetrics, ForecastAdjustmentCandidateV2, ForecastAdjustmentDecision,
    ForecastAdjustmentForecastIdentity, ForecastAdjustmentRawForecastProvenance,
    ForecastAdjustmentRawForecastProvenanceCohortLiteral,
    ForecastAdjustmentRawForecastProvenanceReferenceKindLiteral, ForecastAdjustmentReasonCode,
    ForecastAdjustmentWindCanaryCandidateV1, ForecastAdjustmentWindCanaryMetric,
    ForecastAdjustmentWindCanaryRuntimeBundle, ForecastAdjustmentWindMaintenanceRuntimePackage,
    JsDateOrString, LoadedApiForecastAdjustmentRuntime, LoadedForecastAdjustmentRuntimeV1,
    LoadedForecastAdjustmentRuntimeV1DisabledReasonCodeEnum,
    LoadedForecastAdjustmentWindCanaryRuntime, LoadedForecastAdjustmentWindCanaryRuntimeV1,
    LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledReasonCodeEnum, Nullable,
    WindMaintenanceCandidateRow, WindMaintenanceIncumbentRow,
};

use crate::{
    CurrentProjectionError, CurrentTimeResolver, MaintenanceSourceField,
    MaintenanceSourceProjection, MaintenanceSourceProjectionRows, MaintenanceSourceValue,
};

/// Approved process calendar and ICU/tzdata identity for the numerical model.
pub trait MaintenanceWindModelCalendar {
    /// Resolve the original site-local month, season and daypart for one valid clock.
    fn calendar(&self, valid_at: &str) -> Result<AdmittedWindRowCalendar, CurrentProjectionError>;

    /// Compare the installed model fingerprint to the selected process calendar owner.
    fn fingerprint_matches(&self, icu_version: &str, tzdata_version: &str) -> bool;
}

/// Evaluate the loader-admitted maintenance bundle without granting serving authority.
pub fn evaluate_wind_maintenance_candidate(
    source: &MaintenanceSourceProjection<'_>,
    bundle: &ForecastAdjustmentWindMaintenanceRuntimePackage,
    calendar: &dyn MaintenanceWindModelCalendar,
) -> Result<Vec<WindMaintenanceCandidateRow>, CurrentProjectionError> {
    // enforce the generated package's exact source identity before applying its coefficients
    if bundle.source != bundle.candidate.forecastIdentity {
        return Err(wind_error("wind package source differs"));
    }
    let rows = wind_rows(source)?;
    rows.iter()
        .enumerate()
        .map(|(index, fields)| {
            let row = WindSourceRow::admit(fields, index)?;
            let outcome =
                WindSelection::V2(&bundle.candidate, AdmittedWindV2MetricOrder::Maintenance)
                    .evaluate(fields, &row, calendar)?;
            Ok(WindMaintenanceCandidateRow {
                candidateGustMps: row.gust_output(outcome.gust_adjusted),
                candidateSpeedMps: outcome.speed_adjusted.unwrap_or(row.speed),
                gustWouldApply: !row.gust_disabled && outcome.gust_adjusted.is_some(),
                speedWouldApply: outcome.speed_adjusted.is_some(),
                validAt: row.valid_at.to_owned(),
            })
        })
        .collect()
}

/// Replay the exact already-loaded serving selection on the same original forecast halo.
pub fn evaluate_wind_maintenance_incumbent(
    source: &MaintenanceSourceProjection<'_>,
    runtime: &LoadedApiForecastAdjustmentRuntime,
    time: &dyn CurrentTimeResolver,
    calendar: &dyn MaintenanceWindModelCalendar,
) -> Result<Vec<WindMaintenanceIncumbentRow>, CurrentProjectionError> {
    let rows = wind_rows(source)?;
    let selection =
        WindSelection::incumbent(runtime, source.header().issued_at.epoch_milliseconds, time);
    rows.iter()
        .enumerate()
        .map(|(index, fields)| {
            let row = WindSourceRow::admit(fields, index)?;
            let outcome = selection.evaluate(fields, &row, calendar)?;
            Ok(WindMaintenanceIncumbentRow {
                gustApplied: !row.gust_disabled && outcome.gust_adjusted.is_some(),
                incumbentGustMps: row.gust_output(outcome.gust_adjusted),
                incumbentSpeedMps: outcome.speed_adjusted.unwrap_or(row.speed),
                reasonCode: outcome
                    .reason
                    .map_or(Nullable::Null, |reason| Nullable::Value(reason.to_owned())),
                speedApplied: outcome.speed_adjusted.is_some(),
                validAt: row.valid_at.to_owned(),
            })
        })
        .collect()
}

/// Borrow the one complete schema admitted by the wind projection.
fn wind_rows<'a>(
    source: &'a MaintenanceSourceProjection<'_>,
) -> Result<&'a [[MaintenanceSourceField; 17]], CurrentProjectionError> {
    match source.rows() {
        MaintenanceSourceProjectionRows::Wind(rows) => Ok(rows),
        MaintenanceSourceProjectionRows::Temperature { .. } => {
            Err(wind_error("wind maintenance source differs"))
        }
    }
}

/// One source row with exactly the two retained forecast metrics.
struct WindSourceRow<'a> {
    gust: Option<f64>,
    gust_disabled: bool,
    speed: f64,
    valid_at: &'a str,
}

impl<'a> WindSourceRow<'a> {
    /// Preserve raw metrics and the disabled 49–72 gust output window.
    fn admit(
        fields: &'a [MaintenanceSourceField],
        index: usize,
    ) -> Result<Self, CurrentProjectionError> {
        let speed = metric(fields, "windSpeedMps64")?
            .ok_or_else(|| wind_error("wind maintenance raw speed is missing"))?;
        let gust = metric(fields, "windGustMps64")?;
        let gust_disabled = (48..72).contains(&index);
        // allow a missing gust only in the predecessor's intentionally disabled band
        if gust.is_none() && !gust_disabled {
            return Err(wind_error("wind maintenance raw gust is missing"));
        }
        Ok(Self {
            gust,
            gust_disabled,
            speed,
            valid_at: text(fields, "validAt")?,
        })
    }

    /// Suppress gust regardless of model output during the unavailable source band.
    fn gust_output(&self, adjusted: Option<f64>) -> Nullable<f64> {
        // retain a structural null rather than numeric imputation
        if self.gust_disabled {
            return Nullable::Null;
        }
        adjusted
            .or(self.gust)
            .map_or(Nullable::Null, Nullable::Value)
    }
}

#[derive(Clone, Copy)]
enum WindSelection<'a> {
    Raw(&'static str),
    V2(&'a ForecastAdjustmentCandidateV2, AdmittedWindV2MetricOrder),
    Legacy(AdmittedWindBundleRow<'a>),
}

impl<'a> WindSelection<'a> {
    /// Select the loaded incumbent and evaluate legacy activation once for the issue clock.
    fn incumbent(
        runtime: &'a LoadedApiForecastAdjustmentRuntime,
        issued_ms: i64,
        time: &dyn CurrentTimeResolver,
    ) -> Self {
        match runtime {
            LoadedApiForecastAdjustmentRuntime::Standard(value) => match value {
                LoadedForecastAdjustmentRuntimeV1::Active(active) => {
                    Self::V2(&active.bundle.candidate, AdmittedWindV2MetricOrder::Standard)
                }
                LoadedForecastAdjustmentRuntimeV1::Disabled(disabled) => Self::Raw(wire_reason(
                    LoadedForecastAdjustmentRuntimeV1DisabledReasonCodeEnum::WIRE_VALUES,
                    disabled.reasonCode,
                )),
            },
            LoadedApiForecastAdjustmentRuntime::Wind(value) => match value {
                LoadedForecastAdjustmentWindCanaryRuntime::MaintenanceActive(active) => {
                    Self::V2(&active.bundle.candidate, AdmittedWindV2MetricOrder::Maintenance)
                }
                LoadedForecastAdjustmentWindCanaryRuntime::MaintenanceDisabled(_) => Self::Raw("policy_raw"),
                LoadedForecastAdjustmentWindCanaryRuntime::Legacy { value, .. } => match value {
                    LoadedForecastAdjustmentWindCanaryRuntimeV1::Active(active) => {
                        Self::legacy(&active.bundle, issued_ms, time)
                    }
                    LoadedForecastAdjustmentWindCanaryRuntimeV1::Disabled(disabled) => Self::Raw(
                        wire_reason(LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledReasonCodeEnum::WIRE_VALUES, disabled.reasonCode),
                    ),
                },
            },
        }
    }

    /// Keep v1 expiry exclusive and permanent v2 activation open-ended.
    fn legacy(
        bundle: &'a ForecastAdjustmentWindCanaryRuntimeBundle,
        issued: i64,
        time: &dyn CurrentTimeResolver,
    ) -> Self {
        let (candidate, activated, expiry, authorization, transfer) = match bundle {
            ForecastAdjustmentWindCanaryRuntimeBundle::TimeLimited(bundle) => (
                &bundle.candidate,
                bundle.authorization.activatedAt.as_str(),
                Some(bundle.authorization.expiresAt.as_str()),
                bundle.authorization.authorizationSha256.as_str(),
                bundle.transferReport.transferReportSha256.as_str(),
            ),
            ForecastAdjustmentWindCanaryRuntimeBundle::Permanent(bundle) => (
                &bundle.candidate,
                bundle.authorization.activatedAt.as_str(),
                None,
                bundle.authorization.authorizationSha256.as_str(),
                bundle.transferReport.transferReportSha256.as_str(),
            ),
        };
        // use the loaded version's clock policy without extending any serving authority
        if !epoch_ms(time, activated).is_some_and(|start| issued >= start)
            || expiry.is_some_and(|end| !epoch_ms(time, end).is_some_and(|end| issued < end))
        {
            return Self::Raw("canary_expired");
        }
        Self::Legacy(AdmittedWindBundleRow {
            authorization_sha256: authorization,
            candidate,
            transfer_report_sha256: transfer,
        })
    }

    /// Admit the exact source identity and apply the real selected numerical routine.
    fn evaluate(
        self,
        fields: &[MaintenanceSourceField],
        row: &WindSourceRow<'_>,
        calendar: &dyn MaintenanceWindModelCalendar,
    ) -> Result<WindOutcome, CurrentProjectionError> {
        let (expected, icu, tzdata) = match self {
            Self::Raw(reason) => return Ok(WindOutcome::raw(reason)),
            Self::V2(model, _) => (
                &model.forecastIdentity,
                model.runtimeFingerprint.icuVersion.as_str(),
                model.runtimeFingerprint.tzdataVersion.as_str(),
            ),
            Self::Legacy(bundle) => legacy_source(bundle.candidate),
        };
        let target_lead = number(fields, "modelLeadHours")?;
        // implement the predecessor lead and identity guards before touching the calendar
        if !(1..=168).contains(&target_lead) {
            return Ok(WindOutcome::raw("unsupported_lead"));
        }
        let Some(provenance) = admitted_provenance(fields, target_lead, expected)? else {
            return Ok(WindOutcome::raw("identity_mismatch"));
        };
        let metrics = wind_metrics(row.speed, row.gust);
        let features = AdmittedWindRowFeatures {
            calendar: calendar.calendar(row.valid_at)?,
            calendar_fingerprint_matches: calendar.fingerprint_matches(icu, tzdata),
            metrics: &metrics,
            raw_forecast_provenance: &provenance,
            source_identity: AdmittedWindRowSourceIdentity::Match,
        };
        match self {
            Self::V2(candidate, order) => {
                Ok(evaluate_admitted_wind_v2_row(candidate, features, order).into())
            }
            Self::Legacy(bundle) => {
                legacy_outcome(evaluate_admitted_wind_bundle_row(bundle, features))
            }
            Self::Raw(reason) => Ok(WindOutcome::raw(reason)),
        }
    }
}

/// Borrow the serving-source and process fingerprint of the installed legacy canary.
fn legacy_source(
    candidate: &ForecastAdjustmentWindCanaryCandidateV1,
) -> (&ForecastAdjustmentForecastIdentity, &str, &str) {
    (
        &candidate.servedForecastIdentity,
        &candidate.runtimeFingerprint.icuVersion,
        &candidate.runtimeFingerprint.tzdataVersion,
    )
}

/// Bind exact storage provenance without accepting normalization aliases.
fn admitted_provenance(
    fields: &[MaintenanceSourceField],
    lead: u64,
    expected: &ForecastAdjustmentForecastIdentity,
) -> Result<Option<ForecastAdjustmentRawForecastProvenance>, CurrentProjectionError> {
    let adapter = text(fields, "adapterVersion")?;
    let contract = text(fields, "contractEpoch")?;
    let fingerprint = text(fields, "sourceConfigFingerprint")?;
    let source_key = text(fields, "sourceKey")?;
    let upstream = text(fields, "upstreamModel")?;
    let dataset_matches = match field(fields, "dataset")? {
        MaintenanceSourceValue::Utf16(actual) => {
            actual.iter().copied().eq(expected.dataset.encode_utf16())
        }
        MaintenanceSourceValue::Text(actual) => actual == &expected.dataset,
        _ => false,
    };
    // the fixed cohort and provenance kind are generated TypeScript literals
    if adapter != expected.adapterVersion
        || contract != expected.contractEpoch
        || fingerprint != expected.sourceConfigFingerprint
        || source_key != expected.sourceKey
        || upstream != expected.upstreamModel
        || !dataset_matches
    {
        return Ok(None);
    }
    Ok(Some(ForecastAdjustmentRawForecastProvenance {
        adapterVersion: adapter.to_owned(),
        cohort: ForecastAdjustmentRawForecastProvenanceCohortLiteral,
        contractEpoch: contract.to_owned(),
        dataset: expected.dataset.clone(),
        referenceAt: text(fields, "referenceAt")?.to_owned(),
        referenceKind: ForecastAdjustmentRawForecastProvenanceReferenceKindLiteral,
        sourceConfigFingerprint: fingerprint.to_owned(),
        sourceKey: source_key.to_owned(),
        targetLeadHours: lead,
        upstreamModel: upstream.to_owned(),
        validAt: text(fields, "validAt")?.to_owned(),
    }))
}

/// Only these two fields are populated by the original captured wind metrics.
fn wind_metrics(speed: f64, gust: Option<f64>) -> CanonicalWeatherMetrics {
    CanonicalWeatherMetrics {
        apparentTemperatureC: Nullable::Null,
        blackGlobeTemperatureC: Nullable::Null,
        cloudCoverPercent: Nullable::Null,
        pm25MicrogramsPerCubicMeter: Nullable::Null,
        precipitationMm: Nullable::Null,
        precipitationRateMmPerHour: Nullable::Null,
        pressureHpa: Nullable::Null,
        relativeHumidityPercent: Nullable::Null,
        soilElectricalConductivityMicrosiemensPerCm: Nullable::Null,
        soilMoisturePercent: Nullable::Null,
        solarRadiationWm2: Nullable::Null,
        temperatureC: Nullable::Null,
        uvIndex: Nullable::Null,
        waterLevelM: Nullable::Null,
        windDirectionDegrees: Nullable::Null,
        windGustMps: gust.map_or(Nullable::Null, Nullable::Value),
        windSpeedMps: Nullable::Value(speed),
        wetBulbGlobeTemperatureC: Nullable::Null,
    }
}

/// Compact, authority-free result shared by the two private output projections.
struct WindOutcome {
    gust_adjusted: Option<f64>,
    speed_adjusted: Option<f64>,
    reason: Option<&'static str>,
}

impl WindOutcome {
    /// Keep both metrics raw with the exact predecessor reason string.
    const fn raw(reason: &'static str) -> Self {
        Self {
            gust_adjusted: None,
            speed_adjusted: None,
            reason: Some(reason),
        }
    }
}

impl From<WindV2RowOutcome> for WindOutcome {
    /// Project native candidate numerics without creating a serving decision.
    fn from(outcome: WindV2RowOutcome) -> Self {
        Self {
            gust_adjusted: outcome.gust_adjusted,
            speed_adjusted: outcome.speed_adjusted,
            reason: outcome
                .reason
                .map(|reason| wire_reason(ForecastAdjustmentReasonCode::WIRE_VALUES, reason)),
        }
    }
}

/// Project only the two selected metrics from the original typed canary decision.
fn legacy_outcome(
    decision: ForecastAdjustmentDecision,
) -> Result<WindOutcome, CurrentProjectionError> {
    match decision {
        ForecastAdjustmentDecision::FailRaw(raw) => Ok(WindOutcome::raw(wire_reason(
            ForecastAdjustmentReasonCode::WIRE_VALUES,
            raw.reasonCode,
        ))),
        ForecastAdjustmentDecision::WindCanaryActive(active) => {
            let applied = active.adjustedMetrics.0;
            let value = |metric| {
                applied
                    .iter()
                    .find_map(|(name, value)| (*name == metric).then_some(*value))
            };
            Ok(WindOutcome {
                gust_adjusted: value(ForecastAdjustmentWindCanaryMetric::WindGustMps),
                speed_adjusted: value(ForecastAdjustmentWindCanaryMetric::WindSpeedMps),
                reason: None,
            })
        }
        ForecastAdjustmentDecision::Active(_) => Err(wind_error("wind canary decision differs")),
    }
}

/// Read a closed generated or derived projection field by its canonical key.
fn field<'a>(
    fields: &'a [MaintenanceSourceField],
    name: &str,
) -> Result<&'a MaintenanceSourceValue, CurrentProjectionError> {
    fields
        .iter()
        .find_map(|(key, value)| (*key == name).then_some(value))
        .ok_or_else(|| wind_error("wind maintenance source field is missing"))
}

/// Borrow one required text clock or identity without copying.
fn text<'a>(
    fields: &'a [MaintenanceSourceField],
    name: &str,
) -> Result<&'a str, CurrentProjectionError> {
    match field(fields, name)? {
        MaintenanceSourceValue::Text(value) => Ok(value),
        _ => Err(wind_error("wind maintenance text field differs")),
    }
}

/// Read the already admitted model lead as an integer.
fn number(fields: &[MaintenanceSourceField], name: &str) -> Result<u64, CurrentProjectionError> {
    match field(fields, name)? {
        MaintenanceSourceValue::Number(value) => Ok(*value),
        _ => Err(wind_error("wind maintenance numeric field differs")),
    }
}

/// Decode the source's exact binary64 without decimal conversion.
fn metric(
    fields: &[MaintenanceSourceField],
    name: &str,
) -> Result<Option<f64>, CurrentProjectionError> {
    match field(fields, name)? {
        MaintenanceSourceValue::Null => Ok(None),
        MaintenanceSourceValue::Text(value) => u64::from_str_radix(value, 16)
            .ok()
            .map(f64::from_bits)
            .filter(|value| value.is_finite())
            .map(Some)
            .ok_or_else(|| wind_error("wind maintenance binary64 differs")),
        _ => Err(wind_error("wind maintenance metric field differs")),
    }
}

/// Reuse the current API clock owner for already loaded canary activation material.
fn epoch_ms(time: &dyn CurrentTimeResolver, value: &str) -> Option<i64> {
    time.resolve(&JsDateOrString::String(value.encode_utf16().collect()))
        .ok()
        .map(|instant| instant.epoch_milliseconds)
}

/// Render a generated reason without guessing at enum-to-wire spelling.
fn wire_reason<Reason: PartialEq>(
    reasons: &'static [(&'static str, Reason)],
    reason: Reason,
) -> &'static str {
    reasons
        .iter()
        .find_map(|(wire, known)| (*known == reason).then_some(*wire))
        .unwrap_or("adjustment_error")
}

/// Contain invalid candidate source material in the private evaluation path.
fn wind_error(detail: &str) -> CurrentProjectionError {
    CurrentProjectionError {
        code: "maintenance_model.wind",
        detail: detail.to_owned(),
    }
}
