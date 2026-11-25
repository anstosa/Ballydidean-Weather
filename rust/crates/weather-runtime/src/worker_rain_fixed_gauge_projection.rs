//! Build the original grouped target projection with native Tempest interval replay.

use std::collections::BTreeMap;

use weather_contracts::{
    AdjustmentRainFixedGaugeTargetCaptureBody, AdjustmentRainFixedGaugeTargetCaptureMember,
    AdjustmentRainFixedGaugeTargetClaim, AdjustmentRainFixedGaugeTargetInterval,
    AdjustmentRainFixedGaugeTargetProjection,
    AdjustmentRainFixedGaugeTargetProjectionContractVersionLiteral,
    AdjustmentRainFixedGaugeTargetProjectionFamilyLiteral,
    AdjustmentRainFixedGaugeTargetProjectionProjectionKindLiteral,
    AdjustmentRainFixedGaugeTargetRow, Nullable,
};
use weather_worker::{
    CanonicalWeatherMetrics, RAIN_COLLECTION_STATIONS, RainFixedGaugeBuildResult,
    RainFixedGaugeCapture, RainFixedGaugeCaptureMember, RainFixedGaugeNormalizedRecord,
    RainFixedGaugeProjection, RainFixedGaugeProjectionBuilder, RainFixedGaugeProjectionRow,
    RainFixedGaugeSource, TaskFailure,
};

use crate::maintenance_canonical_json::valid_hash;
use crate::worker_rain_fixed_gauge_wire::target_record;
use crate::worker_rain_gauge_replay::replay_fixed_gauge_target_capture;
use crate::{WorkerRainFixedGaugeIdentityValues, WorkerRainGaugeReplayValues};

const MAX_PROJECTION_BYTES: usize = 3_500 * 1_024;

/// The approved fixed-gauge wire primitives, not an alternate replay implementation.
pub trait WorkerRainFixedGaugeProjectionValues:
    WorkerRainFixedGaugeIdentityValues + WorkerRainGaugeReplayValues
{
    /// Encode one raw provider body in standard original base64 form.
    fn encode_fixed_gauge_base64(&self, raw: &[u8]) -> Option<String>;

    /// Write the original typed projection in its exact ordered JSON with one trailing LF.
    fn encode_fixed_gauge_projection(
        &self,
        projection: &AdjustmentRainFixedGaugeTargetProjection,
    ) -> Option<Vec<u8>>;
}

/// Retained grouped projection builder using one approved provider and canonical value owner.
pub struct NativeWorkerRainFixedGaugeProjection<Values>(Values);

impl<Values> NativeWorkerRainFixedGaugeProjection<Values> {
    /// Retain the approved primitive values across scheduled hours.
    pub const fn new(values: Values) -> Self {
        Self(values)
    }
}

impl<Values: WorkerRainFixedGaugeProjectionValues> RainFixedGaugeProjectionBuilder
    for NativeWorkerRainFixedGaugeProjection<Values>
{
    /// Replay and encode the exact ordered twelve-station group without copying raw bodies.
    fn build_projection(
        &mut self,
        captures: &[RainFixedGaugeCapture],
        sources: &[RainFixedGaugeSource],
        target_cutoff_at: &str,
        valid_at: &str,
    ) -> Result<RainFixedGaugeBuildResult, TaskFailure> {
        let values = &mut self.0;
        let count = RAIN_COLLECTION_STATIONS.len();
        // the pending-hour SQL admits exactly one immutable provider response per gauge
        if captures.len() != count || sources.len() != count {
            return Err(failure());
        }
        let mut rows = Vec::with_capacity(count);
        let mut persisted = Vec::with_capacity(count);
        let mut bodies = BTreeMap::<&str, Vec<&RainFixedGaugeCapture>>::new();
        for ((capture, source), station) in
            captures.iter().zip(sources).zip(RAIN_COLLECTION_STATIONS)
        {
            // require the original dedicated station order before decoding its raw payload
            if source.location_id != station.location_id
                || capture.station_id != station.location_id
                || !valid_hash(&capture.body_sha256)
            {
                return Err(failure());
            }
            let output = replay_fixed_gauge_target_capture(
                values,
                capture,
                station,
                valid_at,
                target_cutoff_at,
            )?;
            let group = bodies.entry(&capture.body_sha256).or_default();
            // equal content addresses must still identify the same exact original body
            if group
                .first()
                .is_some_and(|first| first.body != capture.body)
            {
                return Err(failure());
            }
            group.push(capture);
            let worker_record = RainFixedGaugeNormalizedRecord {
                metrics: target_metrics(output.precipitation_mm, output.temperature_c),
                received_at: capture.completed_at.clone(),
                source_id: source.source_id.clone(),
                valid_at: valid_at.to_owned(),
            };
            let normalized_record =
                target_record(station.location_id, &worker_record).ok_or_else(failure)?;
            let content = values
                .fixed_gauge_record_content_sha256(&normalized_record)
                .filter(|digest| valid_hash(digest.as_str()))
                .ok_or_else(failure)?;
            let member = capture_member(capture);
            let intervals = output
                .intervals
                .into_iter()
                .map(|interval| {
                    Some(AdjustmentRainFixedGaugeTargetInterval {
                        bodySha256: capture.body_sha256.clone(),
                        claimId: capture.claim_id.clone(),
                        completedAt: capture.completed_at.clone(),
                        durationMinutes: interval.minutes,
                        precipitationMm64: binary64(interval.amount)?,
                        temperatureC64: match interval.temperature_c {
                            Some(value) => Nullable::Value(binary64(value)?),
                            None => Nullable::Null,
                        },
                        validAt: interval.valid_at,
                    })
                })
                .collect::<Option<Vec<_>>>()
                .ok_or_else(failure)?;
            rows.push(AdjustmentRainFixedGaugeTargetRow {
                captureMembers: vec![member],
                intervals,
                normalizedRecord: normalized_record,
                stationId: station.location_id,
                storedContentSha256: content.as_str().to_owned(),
            });
            persisted.push(RainFixedGaugeProjectionRow {
                capture_members: vec![RainFixedGaugeCaptureMember {
                    body_sha256: capture.body_sha256.clone(),
                    claim_id: capture.claim_id.clone(),
                    completed_at: capture.completed_at.clone(),
                }],
                normalized_record: worker_record,
                station_id: station.location_id,
                stored_content_sha256: content.as_str().to_owned(),
            });
        }
        // avoid allocating a base64 proof that cannot fit even without its JSON envelope
        let lower_bound = bodies
            .values()
            .try_fold(0_usize, |total, group| {
                total.checked_add(group.first()?.body.len().div_ceil(3).checked_mul(4)?)
            })
            .ok_or_else(failure)?;
        if lower_bound > MAX_PROJECTION_BYTES {
            return Ok(oversized(valid_at));
        }
        let capture_bodies = bodies
            .into_iter()
            .map(|(digest, mut group)| {
                group.sort_unstable_by(|left, right| {
                    left.completed_at
                        .cmp(&right.completed_at)
                        .then(left.claim_id.cmp(&right.claim_id))
                });
                Some(AdjustmentRainFixedGaugeTargetCaptureBody {
                    bodyBase64: values.encode_fixed_gauge_base64(&group.first()?.body)?,
                    bodySha256: digest.to_owned(),
                    claims: group
                        .into_iter()
                        .map(|capture| AdjustmentRainFixedGaugeTargetClaim {
                            claimId: capture.claim_id.clone(),
                            completedAt: capture.completed_at.clone(),
                            stationId: capture.station_id,
                            windowEndExclusive: capture.window_end_exclusive.clone(),
                            windowStart: capture.window_start.clone(),
                        })
                        .collect(),
                })
            })
            .collect::<Option<Vec<_>>>()
            .ok_or_else(failure)?;
        let typed = AdjustmentRainFixedGaugeTargetProjection {
            captureBodies: capture_bodies,
            contractVersion: AdjustmentRainFixedGaugeTargetProjectionContractVersionLiteral,
            family: AdjustmentRainFixedGaugeTargetProjectionFamilyLiteral,
            logicalReceivedAt: target_cutoff_at.to_owned(),
            projectionKind: AdjustmentRainFixedGaugeTargetProjectionProjectionKindLiteral,
            rows,
            validAt: valid_at.to_owned(),
        };
        let bytes = values
            .encode_fixed_gauge_projection(&typed)
            .ok_or_else(failure)?;
        // classify only the exact original canonical byte ceiling as permanently oversized
        if bytes.len() > MAX_PROJECTION_BYTES {
            return Ok(oversized(valid_at));
        }
        Ok(RainFixedGaugeBuildResult::Complete {
            bytes,
            projection: RainFixedGaugeProjection {
                logical_received_at: target_cutoff_at.to_owned(),
                rows: persisted,
                valid_at: valid_at.to_owned(),
            },
        })
    }
}

/// Preserve one original capture member in exact target projection shape.
fn capture_member(capture: &RainFixedGaugeCapture) -> AdjustmentRainFixedGaugeTargetCaptureMember {
    AdjustmentRainFixedGaugeTargetCaptureMember {
        bodySha256: capture.body_sha256.clone(),
        claimId: capture.claim_id.clone(),
        completedAt: capture.completed_at.clone(),
    }
}

/// Encode exactly the binary64 carried by the normalized fixed-hour interval.
fn binary64(value: f64) -> Option<String> {
    value
        .is_finite()
        .then(|| format!("{:016x}", value.to_bits()))
}

/// Emit only the native target amount, rate, and independently recovered temperature.
fn target_metrics(amount: Option<f64>, temperature: Option<f64>) -> CanonicalWeatherMetrics {
    CanonicalWeatherMetrics {
        apparent_temperature_c: None,
        black_globe_temperature_c: None,
        cloud_cover_percent: None,
        pm25_micrograms_per_cubic_meter: None,
        precipitation_mm: amount,
        precipitation_rate_mm_per_hour: amount,
        pressure_hpa: None,
        relative_humidity_percent: None,
        soil_electrical_conductivity_microsiemens_per_cm: None,
        soil_moisture_percent: None,
        solar_radiation_wm2: None,
        temperature_c: temperature,
        uv_index: None,
        water_level_m: None,
        wind_direction_degrees: None,
        wind_gust_mps: None,
        wind_speed_mps: None,
        wet_bulb_globe_temperature_c: None,
    }
}

/// Preserve one original value-free oversized result.
fn oversized(valid_at: &str) -> RainFixedGaugeBuildResult {
    RainFixedGaugeBuildResult::Gap {
        logical_hour_at: valid_at.to_owned(),
    }
}

/// Report target construction errors without provider bodies.
fn failure() -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: "rain_fixed_gauge_projection_invalid".to_owned(),
    }
}
