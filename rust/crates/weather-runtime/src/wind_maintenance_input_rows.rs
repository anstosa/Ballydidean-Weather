//! Project only final-training metrics after the admitted cutoff has selected row indices.

use weather_contracts::{
    ForecastAdjustmentForecastIdentity, ForecastAdjustmentForecastIdentityCohortLiteral,
    ForecastAdjustmentForecastIdentityReferenceKindLiteral, ForecastObservationStationKey,
    JsonValue, Nullable, SanitizedTrainingMetrics,
};
use weather_worker::wind_maintenance_development::{
    AdmittedWindForecastTrainingRow, AdmittedWindStationTrainingRow, AdmittedWindTrainingRow,
};

use crate::ApiMaintenanceShadowEvidenceValues;
use crate::maintenance_canonical_json::{field, text, valid_hash};
use crate::wind_maintenance_input::{
    CutoffClock, WindFitInputCalendar, WindFitInputError, WindFitLocalFeatures, fit_clock,
};

pub(super) struct WindFitRowHeader {
    pub(super) features: WindFitLocalFeatures,
    pub(super) received_at: Option<CutoffClock>,
    valid_at: String,
    valid_at_ms: i64,
}

pub(super) struct WindFitTrainingRow {
    header: WindFitRowHeader,
    kind: WindFitTrainingKind,
}

enum WindFitTrainingKind {
    Anchor,
    Station {
        station_key: ForecastObservationStationKey,
        metrics: SanitizedTrainingMetrics,
    },
    Retrieval {
        digest: String,
        metrics: SanitizedTrainingMetrics,
        reference_at: String,
        reference_at_ms: i64,
        lead: u16,
    },
}

impl WindFitTrainingRow {
    /// Borrow the current final-training row in the same position as the original phase.
    pub(super) fn admitted(&self) -> AdmittedWindTrainingRow<'_> {
        match &self.kind {
            WindFitTrainingKind::Anchor => AdmittedWindTrainingRow::Other,
            WindFitTrainingKind::Station {
                station_key,
                metrics,
            } => AdmittedWindTrainingRow::Station(AdmittedWindStationTrainingRow {
                physical_station_key: *station_key,
                valid_at_ms: self.header.valid_at_ms,
                metrics,
            }),
            WindFitTrainingKind::Retrieval {
                digest,
                metrics,
                reference_at,
                reference_at_ms,
                lead,
            } => AdmittedWindTrainingRow::Retrieval(AdmittedWindForecastTrainingRow {
                content_sha256: digest,
                metrics,
                calendar: self.header.features.calendar,
                local_date: &self.header.features.local_date,
                reference_at,
                reference_at_ms: *reference_at_ms,
                target_lead_hours: *lead,
                valid_at: &self.header.valid_at,
                valid_at_ms: self.header.valid_at_ms,
            }),
        }
    }
}

/// Read availability only; do not inspect confirmation-era metrics or identities.
pub(super) fn admission_headers(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    calendar: &impl WindFitInputCalendar,
    rows: &[JsonValue],
) -> Result<Vec<WindFitRowHeader>, WindFitInputError> {
    rows.iter()
        .map(|row| {
            let valid_at = text(row, "validAt")?;
            let valid_at_ms = values.canonical_utc_epoch_milliseconds(&valid_at)?;
            let features = calendar.wind_fit_local_features(&valid_at, valid_at_ms)?;
            Some(WindFitRowHeader {
                features,
                valid_at,
                valid_at_ms,
                received_at: fit_clock(values, text(row, "receivedAt")),
            })
        })
        .collect::<Option<Vec<_>>>()
        .ok_or(WindFitInputError::Row)
}

/// Keep only selected phase rows, their clean numeric wind columns and first served identity.
pub(super) fn training_projection(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    rows: Vec<JsonValue>,
    headers: Vec<WindFitRowHeader>,
    selected: &[usize],
) -> Result<
    (
        Vec<WindFitTrainingRow>,
        Option<ForecastAdjustmentForecastIdentity>,
    ),
    WindFitInputError,
> {
    let mut output = Vec::with_capacity(selected.len());
    let mut has_retrieval = false;
    let mut identity = None;
    let mut next = selected.iter().copied().peekable();
    // retain the exact canonical source row order while moving only selected headers
    for (index, (row, mut header)) in rows.into_iter().zip(headers).enumerate() {
        // leave all discarded rows unprojected
        if next.peek() != Some(&index) {
            continue;
        }
        next.next();
        // receipt admission is complete and no later numerical step reads the original clock
        header.received_at = None;
        let kind = match text(&row, "recordKind").as_deref() {
            Some("station_hour") => match station_key(&row) {
                Some(station_key) => WindFitTrainingKind::Station {
                    station_key,
                    metrics: read_wind_metrics(&row).ok_or(WindFitInputError::Row)?,
                },
                // the original station catalog ignores unknown physical station rows
                None => WindFitTrainingKind::Anchor,
            },
            Some("legacy_v4_retrieval_snapshot") => {
                // collect identity from the first served cohort row regardless of metric values
                if !has_retrieval {
                    has_retrieval = true;
                    identity = first_forecast_identity(&row);
                }
                let metrics = read_wind_metrics(&row).ok_or(WindFitInputError::Row)?;
                // original numeric selection does not inspect references for both-null rows
                if matches!(metrics.windGustMps, Nullable::Null)
                    && matches!(metrics.windSpeedMps, Nullable::Null)
                {
                    WindFitTrainingKind::Anchor
                } else {
                    retrieval(values, &row, metrics).ok_or(WindFitInputError::Row)?
                }
            }
            _ => WindFitTrainingKind::Anchor,
        };
        output.push(WindFitTrainingRow { header, kind });
    }
    Ok((output, identity))
}

/// Bind an original legacy-v4 retrieval row before jitter ordering.
fn retrieval(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    row: &JsonValue,
    metrics: SanitizedTrainingMetrics,
) -> Option<WindFitTrainingKind> {
    // preserve the exact original retrieval reference kind
    if text(row, "referenceKind").as_deref() != Some("retrieval_snapshot") {
        return None;
    }
    let reference_at = text(row, "referenceAt")?;
    let reference_at_ms = values.canonical_utc_epoch_milliseconds(&reference_at)?;
    let JsonValue::Number(lead) = field(row, "targetLeadHours")? else {
        return None;
    };
    // convert only a safe original whole target lead
    if !(1.0..=168.0).contains(lead) || lead.fract() != 0.0 {
        return None;
    }
    let digest = first_text(row, "contentHashes").filter(|hash| valid_hash(hash))?;
    Some(WindFitTrainingKind::Retrieval {
        digest,
        metrics,
        reference_at,
        reference_at_ms,
        lead: *lead as u16,
    })
}

/// Recognize the existing physical station catalog keys.
fn station_key(row: &JsonValue) -> Option<ForecastObservationStationKey> {
    let key = text(row, "physicalStationKey")?;
    ForecastObservationStationKey::WIRE_VALUES
        .iter()
        .find_map(|(name, value)| (*name == key).then_some(*value))
}

/// Project the two maintained channels from the generated sanitized metric contract.
fn read_wind_metrics(row: &JsonValue) -> Option<SanitizedTrainingMetrics> {
    let metrics = field(row, "metrics")?;
    Some(SanitizedTrainingMetrics {
        relativeHumidityPercent: Nullable::Null,
        temperatureC: Nullable::Null,
        windDirectionDegrees: Nullable::Null,
        windGustMps: read_numeric_metric(metrics, "windGustMps")?,
        windSpeedMps: read_numeric_metric(metrics, "windSpeedMps")?,
    })
}

/// Admit finite original scalars and explicit JSON nulls without coercion.
fn read_numeric_metric(metrics: &JsonValue, name: &str) -> Option<Nullable<f64>> {
    match field(metrics, name)? {
        JsonValue::Null => Some(Nullable::Null),
        JsonValue::Number(value) if value.is_finite() => Some(Nullable::Value(*value)),
        _ => None,
    }
}

/// Take the retained served identity exactly as original candidate construction does.
fn first_forecast_identity(row: &JsonValue) -> Option<ForecastAdjustmentForecastIdentity> {
    Some(ForecastAdjustmentForecastIdentity {
        adapterVersion: text(row, "adapterVersion")?,
        cohort: ForecastAdjustmentForecastIdentityCohortLiteral,
        contractEpoch: text(row, "contractEpoch")?,
        dataset: text(row, "dataset")?,
        referenceKind: ForecastAdjustmentForecastIdentityReferenceKindLiteral,
        sourceConfigFingerprint: first_text(row, "sourceConfigFingerprints")?,
        sourceKey: first_text(row, "sourceKeys")?,
        upstreamModel: text(row, "upstreamModel")?,
    })
}

/// Read only the first member of an admitted source identity or digest array.
fn first_text(row: &JsonValue, name: &str) -> Option<String> {
    let JsonValue::Array(values) = field(row, name)? else {
        return None;
    };
    let JsonValue::String(units) = values.first()? else {
        return None;
    };
    String::from_utf16(units).ok()
}
