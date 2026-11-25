//! Replay of retained causal rain features through an admitted portable artifact.

use weather_adjustment::RainHurdleWindPortableModel;
use weather_contracts::{
    Nullable, RainHurdleWindFeatureProjection, RainHurdleWindPortableArtifact,
    RainWindPerformanceHour, RainWindPerformanceHourOccurrenceProbabilities,
    RainWindPerformanceResult, RainWindPredictionHourReasonCodeEnum,
};

use crate::TaskFailure;

const OUTPUT_ROW_COUNT: usize = 23;

/// Approved local-calendar projection required only by portable v2 artifacts.
pub trait RainPortableReplayCalendar {
    /// Resolve the Los Angeles month only when the UTC target round-trips canonically.
    fn local_month(&mut self, valid_at: &str) -> Option<u8>;
}

/// Score the fixed ordinal control without supplying a target clock or serving fallbacks.
pub fn score_rain_control_ordinals(
    projection: &RainHurdleWindFeatureProjection,
    artifact: &RainHurdleWindPortableArtifact,
) -> Result<Vec<f64>, TaskFailure> {
    let model = RainHurdleWindPortableModel::new(artifact)
        .map_err(|_| replay_failure("rain control ordinal artifact differs"))?;
    // the original v2 portable evaluator rejects a missing target clock on every arm
    projection
        .rows
        .iter()
        .map(|row| {
            model
                .predict(&row.features, None)
                .map(|value| value.correctedPrecipitationMm)
                .map_err(|_| replay_failure("rain control ordinal evaluation failed"))
        })
        .collect()
}

/// Replay one already-built causal feature projection without rebuilding its inputs.
pub fn replay_rain_maintenance_candidate(
    projection: &RainHurdleWindFeatureProjection,
    model_sha256: &str,
    artifact: &RainHurdleWindPortableArtifact,
    calendar: &mut impl RainPortableReplayCalendar,
) -> Result<RainWindPerformanceResult, TaskFailure> {
    let model_month = artifact_model_month(artifact);
    // bind output to one exact model identity and portable artifact month
    if !valid_sha256(model_sha256) || !valid_model_month(model_month) {
        return Err(replay_failure("rain parity runtime identity is invalid"));
    }
    let model = RainHurdleWindPortableModel::new(artifact)
        .map_err(|_| replay_failure("rain model artifact schema mismatch"))?;
    // require the fixed fitted geometry before any model evaluation
    if projection.rows.len() != OUTPUT_ROW_COUNT || !valid_decision_hour(&projection.decisionAt) {
        return Err(replay_failure(
            "rain feature projection geometry is invalid",
        ));
    }

    let mut hours = Vec::with_capacity(OUTPUT_ROW_COUNT);
    // retain source order across the exact trained output horizon
    for row in &projection.rows {
        // phase-ineligible targets never invoke the calendar or model
        if row.rawTargetHourTemperatureC <= 2.0 {
            hours.push(fallback_hour(
                row,
                RainWindPredictionHourReasonCodeEnum::PhaseUnsupported,
            ));
            continue;
        }
        // contain one row-level scoring failure without discarding sibling hours
        match model.predict_with_local_month(&row.features, || {
            // require the original v2 target shape before consulting the exact calendar owner
            if !valid_utc_millisecond(&row.validAt) {
                return None;
            }
            calendar.local_month(&row.validAt)
        }) {
            Ok(prediction) => hours.push(RainWindPerformanceHour {
                validAt: row.validAt.clone(),
                modelLeadHours: row.modelLeadHours,
                rawPrecipitationMm: row.rawPrecipitationMm,
                correctedPrecipitationMm: prediction.correctedPrecipitationMm,
                applied: true,
                reasonCode: Nullable::Null,
                occurrenceProbabilities: Nullable::Value(
                    RainWindPerformanceHourOccurrenceProbabilities {
                        atLeast0_1: prediction.occurrenceProbabilityAtLeast0_1,
                        atLeast1_0: prediction.occurrenceProbabilityAtLeast1_0,
                        atLeast2_5: prediction.occurrenceProbabilityAtLeast2_5,
                    },
                ),
                positiveAmountMm: Nullable::Value(prediction.positiveAmountMm),
            }),
            Err(_) => hours.push(fallback_hour(
                row,
                RainWindPredictionHourReasonCodeEnum::PredictionInvalid,
            )),
        }
    }
    Ok(RainWindPerformanceResult {
        modelSha256: model_sha256.to_owned(),
        modelMonth: model_month.to_owned(),
        decisionAt: projection.decisionAt.clone(),
        hours,
    })
}

/// Render one unchanged row after phase or prediction refusal.
fn fallback_hour(
    row: &weather_contracts::RainHurdleWindFeatureRow,
    reason: RainWindPredictionHourReasonCodeEnum,
) -> RainWindPerformanceHour {
    RainWindPerformanceHour {
        validAt: row.validAt.clone(),
        modelLeadHours: row.modelLeadHours,
        rawPrecipitationMm: row.rawPrecipitationMm,
        correctedPrecipitationMm: row.rawPrecipitationMm,
        applied: false,
        reasonCode: Nullable::Value(reason),
        occurrenceProbabilities: Nullable::Null,
        positiveAmountMm: Nullable::Null,
    }
}

/// Borrow the portable artifact's retained fitted month.
fn artifact_model_month(artifact: &RainHurdleWindPortableArtifact) -> &str {
    match artifact {
        RainHurdleWindPortableArtifact::V1(artifact) => artifact.modelMonth.as_str(),
        RainHurdleWindPortableArtifact::V2(artifact) => artifact.modelMonth.as_str(),
    }
}

/// Recognize one lowercase hexadecimal SHA-256 identity.
fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

/// Recognize the exact fitted year-month spelling.
fn valid_model_month(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 7
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'-'
        && matches!(
            &bytes[5..],
            b"01"
                | b"02"
                | b"03"
                | b"04"
                | b"05"
                | b"06"
                | b"07"
                | b"08"
                | b"09"
                | b"10"
                | b"11"
                | b"12"
        )
}

/// Recognize the replay projection's fixed top-of-hour decision spelling.
fn valid_decision_hour(value: &str) -> bool {
    let bytes = value.as_bytes();
    valid_utc_millisecond(value) && &bytes[14..] == b"00:00.000Z"
}

/// Recognize the exact UTC-millisecond lexical shape required before calendar use.
fn valid_utc_millisecond(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 24
        && [0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18, 20, 21, 22]
            .iter()
            .all(|index| bytes[*index].is_ascii_digit())
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b'T'
        && bytes[13] == b':'
        && bytes[16] == b':'
        && bytes[19] == b'.'
        && bytes[23] == b'Z'
}

/// Build one bounded worker failure without provider attempts.
fn replay_failure(code: &str) -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: code.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use weather_contracts::{
        RainHurdleWindFeatureRow, RainHurdleWindPortableArtifactV1Heads,
        RainHurdleWindPortableArtifactV1HeadsNativeHead,
        RainHurdleWindPortableArtifactV1HeadsObjectiveEnum, RainHurdleWindPortableArtifactV1Rules,
        RainHurdleWindPortableArtifactV2, RainHurdleWindPortableArtifactV2ContractVersionLiteral,
        RainHurdleWindProjectionId,
    };

    use super::*;

    const FEATURE_COUNT: usize = 107;
    const TREE_COUNT: usize = 160;

    struct Calendar {
        calls: Vec<String>,
    }

    impl RainPortableReplayCalendar for Calendar {
        /// Return no month for the one ordinary unresolved target.
        fn local_month(&mut self, valid_at: &str) -> Option<u8> {
            self.calls.push(valid_at.to_owned());
            // model one unresolved valid target while admitting every sibling
            (valid_at != "2026-11-01T01:00:00.000Z").then_some(11)
        }
    }

    /// Build one valid single-leaf portable tree.
    fn tree() -> [Vec<f64>; 5] {
        [vec![0.0], vec![0.0], vec![-1.0], vec![-1.0], vec![0.0]]
    }

    /// Build one exact portable head with the installed artifact tree count.
    fn head(
        objective: RainHurdleWindPortableArtifactV1HeadsObjectiveEnum,
    ) -> RainHurdleWindPortableArtifactV1HeadsNativeHead {
        RainHurdleWindPortableArtifactV1HeadsNativeHead {
            objective,
            baseScore: 0.5,
            trees: (0..TREE_COUNT).map(|_| tree()).collect(),
        }
    }

    /// Build one admitted-shape v2 artifact for replay.
    fn artifact() -> RainHurdleWindPortableArtifact {
        RainHurdleWindPortableArtifact::V2(RainHurdleWindPortableArtifactV2 {
            modelMonth: "2026-11".to_owned(),
            featureNames: (0..FEATURE_COUNT)
                .map(|index| format!("feature_{index}"))
                .collect(),
            rules: vec![
                RainHurdleWindPortableArtifactV1Rules {
                    threshold: 0.1,
                    cutoff: Nullable::Value(0.5),
                },
                RainHurdleWindPortableArtifactV1Rules {
                    threshold: 1.0,
                    cutoff: Nullable::Value(0.5),
                },
                RainHurdleWindPortableArtifactV1Rules {
                    threshold: 2.5,
                    cutoff: Nullable::Value(0.5),
                },
            ],
            categoryScales: vec![1.0, 1.0, 1.0],
            heads: RainHurdleWindPortableArtifactV1Heads {
                atLeast0_1: head(
                    RainHurdleWindPortableArtifactV1HeadsObjectiveEnum::BinaryLogistic,
                ),
                atLeast1_0: head(
                    RainHurdleWindPortableArtifactV1HeadsObjectiveEnum::BinaryLogistic,
                ),
                atLeast2_5: head(
                    RainHurdleWindPortableArtifactV1HeadsObjectiveEnum::BinaryLogistic,
                ),
                amount: head(RainHurdleWindPortableArtifactV1HeadsObjectiveEnum::RegGamma),
            },
            contractVersion: RainHurdleWindPortableArtifactV2ContractVersionLiteral,
            projectionId: RainHurdleWindProjectionId::R0ExactRefit,
        })
    }

    /// Build the complete 23-row replay horizon with one phase and one calendar refusal.
    fn projection() -> RainHurdleWindFeatureProjection {
        RainHurdleWindFeatureProjection {
            decisionAt: "2026-11-01T00:00:00.000Z".to_owned(),
            rows: (0..OUTPUT_ROW_COUNT)
                .map(|index| {
                    let mut features = vec![0.0; FEATURE_COUNT];
                    features[5] = 0.0;
                    RainHurdleWindFeatureRow {
                        features,
                        modelLeadHours: (index + 9) as f64,
                        rawPrecipitationMm: 0.0,
                        rawTargetHourTemperatureC: if index == 0 { 2.0 } else { 3.0 },
                        validAt: format!("2026-11-01T{index:02}:00:00.000Z"),
                    }
                })
                .collect(),
            runInitializedAt: "2026-10-31T15:00:00.000Z".to_owned(),
        }
    }

    #[test]
    fn replay_preserves_phase_calendar_refusal_and_success_rows() {
        let mut calendar = Calendar { calls: Vec::new() };
        let mut input = projection();
        input.rows[3].features.pop();
        let result =
            replay_rain_maintenance_candidate(&input, &"a".repeat(64), &artifact(), &mut calendar)
                .expect("valid portable replay");

        assert_eq!(result.hours.len(), OUTPUT_ROW_COUNT);
        assert_eq!(
            result.hours[0].reasonCode,
            Nullable::Value(RainWindPredictionHourReasonCodeEnum::PhaseUnsupported)
        );
        assert_eq!(
            result.hours[1].reasonCode,
            Nullable::Value(RainWindPredictionHourReasonCodeEnum::PredictionInvalid)
        );
        assert!(result.hours[2].applied);
        assert_eq!(result.hours[2].correctedPrecipitationMm, 2.5);
        assert_eq!(
            result.hours[3].reasonCode,
            Nullable::Value(RainWindPredictionHourReasonCodeEnum::PredictionInvalid)
        );
        assert_eq!(calendar.calls.len(), OUTPUT_ROW_COUNT - 2);
        assert!(!calendar.calls.contains(&result.hours[0].validAt));
        assert!(!calendar.calls.contains(&result.hours[3].validAt));
        // require the original whole-hour decision bytes before attempting any row
        input.decisionAt = "2026-11-01T00:00:00.001Z".to_owned();
        calendar.calls.clear();
        assert!(
            replay_rain_maintenance_candidate(&input, &"a".repeat(64), &artifact(), &mut calendar)
                .is_err()
        );
        assert!(calendar.calls.is_empty());
    }
}
