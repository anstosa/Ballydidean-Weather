//! Reconstruct the exact prior-hour rain persistence member without future observations.

use weather_contracts::{JsonValue, Nullable};
use weather_worker::{
    RAIN_COLLECTION_STATIONS, RainAdjustmentCaptureKind, RainAdjustmentEvaluation,
    RainAdjustmentPendingInput,
};

use crate::ApiMaintenanceShadowEvidenceValues;
use crate::maintenance_canonical_json::valid_hash;
use crate::worker_rain_shadow::{binary64, object, string};

const HOUR_MS: i64 = 3_600_000;
const OVERLAP_MS: i64 = 125 * 60_000;
// exact binary64 weights of the original compiled TS fixed catalog in station order
const WEIGHTS: [f64; 12] = [
    f64::from_bits(0x3feac6b3c3e9617f),
    f64::from_bits(0x3fea359b380542db),
    f64::from_bits(0x3fe8e968b3225631),
    f64::from_bits(0x3fe8cdec5703a511),
    f64::from_bits(0x3fe57d6bda81c16d),
    f64::from_bits(0x3fe5757fd13c391e),
    f64::from_bits(0x3fe307ef06f37997),
    f64::from_bits(0x3fde1225a651824a),
    f64::from_bits(0x3fcafe7688724d40),
    f64::from_bits(0x3fca49b09d235947),
    f64::from_bits(0x3fc812ce46bfbc06),
    f64::from_bits(0x3fc2b2faab23c3d3),
];

/// Canonical prior-hour target and the numerical value reused by each control row.
pub struct WorkerRainPersistenceTarget {
    pub document: JsonValue,
    pub prediction_mm: Option<f64>,
    pub member_sha256: String,
}

/// Bind every overlapping immutable gauge capture, then select the supported weighted median.
pub fn build_worker_rain_persistence_target(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    pending: &RainAdjustmentPendingInput,
    evaluation: &RainAdjustmentEvaluation,
    valid_at: &str,
) -> Option<WorkerRainPersistenceTarget> {
    let decision_at = &evaluation.run.decision_at;
    let decision = values.canonical_utc_epoch_milliseconds(decision_at)?;
    let target = decision.checked_sub(HOUR_MS)?;
    let earliest = decision.checked_sub(OVERLAP_MS)?;
    // bind the admitted caller clock to the exact original prior target hour
    if values.canonical_utc_epoch_milliseconds(valid_at)? != target {
        return None;
    }
    let mut available = Vec::with_capacity(WEIGHTS.len());
    let mut rows = Vec::with_capacity(WEIGHTS.len());
    // preserve all twelve fixed gauges, including rows with no usable amount
    for (index, station) in RAIN_COLLECTION_STATIONS.iter().enumerate() {
        let hour = evaluation
            .causal_input
            .stationHours
            .iter()
            .rev()
            .find(|row| row.hourAt == valid_at && row.stationId == station.location_id as f64);
        let precipitation = hour.and_then(|row| match row.precipitationMm {
            Nullable::Null => None,
            Nullable::Value(amount) => Some(amount),
        });
        let received_at = hour.map(|row| row.receivedAt.as_str());
        let mut members = Vec::new();
        // retain all original overlapping receipts, not only those chosen by tiling
        for capture in &pending.captures {
            let RainAdjustmentCaptureKind::Station {
                station_id,
                window_start,
                window_end_exclusive,
            } = &capture.kind
            else {
                continue;
            };
            let identity = &capture.identity;
            // exclude any station or provider response outside the causal interval
            if *station_id != station.location_id
                || identity.completed_at_ms > decision
                || values.canonical_utc_epoch_milliseconds(window_start)? > target
                || values.canonical_utc_epoch_milliseconds(window_end_exclusive)? <= earliest
            {
                continue;
            }
            // close the original immutable address and canonical receipt clock
            if !valid_hash(&identity.body_sha256)
                || !(1..=128).contains(&identity.claim_id.encode_utf16().count())
                || values.canonical_utc_epoch_milliseconds(&identity.completed_at)? > decision
            {
                return None;
            }
            members.push(identity);
        }
        members.sort_unstable_by(|left, right| {
            left.completed_at
                .cmp(&right.completed_at)
                .then(left.claim_id.cmp(&right.claim_id))
        });
        // duplicate immutable addresses and unattested receipt clocks are invalid
        if members.windows(2).any(|pair| {
            pair[0].completed_at == pair[1].completed_at && pair[0].claim_id == pair[1].claim_id
        }) || received_at
            .is_some_and(|clock| !members.iter().any(|member| member.completed_at == clock))
        {
            return None;
        }
        let precipitation64 = match precipitation {
            Some(amount) if amount >= 0.0 && amount.is_finite() && received_at.is_some() => {
                available.push((index, amount));
                binary64(amount)?
            }
            Some(_) => return None,
            None => JsonValue::Null,
        };
        let captures = members
            .iter()
            .map(|member| {
                object([
                    ("bodySha256", string(&member.body_sha256)),
                    ("claimId", string(&member.claim_id)),
                    ("completedAt", string(&member.completed_at)),
                ])
            })
            .collect();
        rows.push(object([
            ("captureMembers", JsonValue::Array(captures)),
            ("precipitationMm64", precipitation64),
            ("receivedAt", received_at.map_or(JsonValue::Null, string)),
            ("stationId", JsonValue::Number(station.location_id as f64)),
        ]));
    }
    let prediction = fixed_gauge_weighted_median(&mut available);
    let mut unsigned = object([
        (
            "contractVersion",
            string("rain-maintenance-persistence-target/v1"),
        ),
        ("decisionAt", string(decision_at)),
        (
            "prediction64",
            prediction.map_or(JsonValue::Null, |value| {
                // the weighted median is selected from already admitted finite station values
                string(&format!("{:016x}", value.to_bits()))
            }),
        ),
        (
            "reason",
            string(if prediction.is_some() {
                "causal_target"
            } else {
                "raw_fallback_unavailable"
            }),
        ),
        ("rowCount", JsonValue::Number(12.0)),
        ("rows", JsonValue::Array(rows)),
        ("validAt", string(valid_at)),
    ]);
    let bytes = values.encode_shadow_sorted_json(&unsigned)?;
    let digest = values
        .sha256(&bytes)
        .filter(|digest| valid_hash(digest.as_str()))?;
    let member_sha256 = digest.as_str().to_owned();
    let JsonValue::Object(fields) = &mut unsigned else {
        return None;
    };
    // retain the source-defined field order used in the surrounding control projection
    fields.insert(
        6,
        (
            "targetMemberSha256".encode_utf16().collect(),
            string(&member_sha256),
        ),
    );
    Some(WorkerRainPersistenceTarget {
        document: unsigned,
        prediction_mm: prediction,
        member_sha256,
    })
}

/// Select the original lower tie point only with three gauges including a near station.
pub(crate) fn fixed_gauge_weighted_median(values: &mut [(usize, f64)]) -> Option<f64> {
    // insufficient support is a true missing persistence prediction, never a dry gauge
    if values.len() < 3 || !values.iter().any(|(index, _)| *index < 3) {
        return None;
    }
    values.sort_unstable_by(|(left, a), (right, b)| {
        // finite values compare like JavaScript including signed zero, then use station id
        a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal).then(
            RAIN_COLLECTION_STATIONS[*left]
                .location_id
                .cmp(&RAIN_COLLECTION_STATIONS[*right].location_id),
        )
    });
    let half = values.iter().map(|(index, _)| WEIGHTS[*index]).sum::<f64>() / 2.0;
    let mut cumulative = 0.0;
    // keep the first sorted sample reaching the exact half-mass boundary
    for &(index, value) in values.iter() {
        cumulative += WEIGHTS[index];
        if cumulative >= half {
            return Some(value);
        }
    }
    None
}

#[cfg(test)]
#[path = "worker_rain_persistence_test.rs"]
mod tests;
