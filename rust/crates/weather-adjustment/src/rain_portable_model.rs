//! Typed portable v1 and v2 rain-hurdle model evaluation.

use weather_contracts::{
    RainHurdleWindFeaturePerformance, RainHurdleWindPortableArtifact, RainHurdleWindProjectionId,
};

use crate::{RainHurdleWindInferenceError, RainHurdleWindV1Model};

/// Borrowed validated portable artifact for repeated native inference.
#[derive(Debug)]
pub struct RainHurdleWindPortableModel<'a> {
    native: RainHurdleWindV1Model<'a>,
    projection: Option<RainHurdleWindProjectionId>,
}

impl<'a> RainHurdleWindPortableModel<'a> {
    /// Validate one typed v1 or v2 artifact's shared numerical tables.
    pub fn new(
        artifact: &'a RainHurdleWindPortableArtifact,
    ) -> Result<Self, RainHurdleWindInferenceError> {
        // borrow the common native tables while retaining the v2 projection identity
        let (heads, rules, category_scales, projection) = match artifact {
            RainHurdleWindPortableArtifact::V1(artifact) => (
                &artifact.heads,
                artifact.rules.as_slice(),
                artifact.categoryScales.as_slice(),
                None,
            ),
            RainHurdleWindPortableArtifact::V2(artifact) => (
                &artifact.heads,
                artifact.rules.as_slice(),
                artifact.categoryScales.as_slice(),
                Some(artifact.projectionId),
            ),
        };
        Ok(Self {
            native: RainHurdleWindV1Model::new(heads, rules, category_scales)?,
            projection,
        })
    }

    /// Report whether the caller must supply the already resolved Los Angeles month.
    pub const fn requires_local_month(&self) -> bool {
        self.projection.is_some()
    }

    /// Evaluate one feature row with the selected portable projection semantics.
    pub fn predict(
        &self,
        features: &[f32],
        local_month: Option<u8>,
    ) -> Result<RainHurdleWindFeaturePerformance, RainHurdleWindInferenceError> {
        self.predict_with_local_month(features, || local_month)
    }

    /// Resolve the v2 clock only after the original native feature and raw-amount checks.
    pub fn predict_with_local_month(
        &self,
        features: &[f32],
        local_month: impl FnOnce() -> Option<u8>,
    ) -> Result<RainHurdleWindFeaturePerformance, RainHurdleWindInferenceError> {
        let Some(projection) = self.projection else {
            return self.native.predict(features);
        };
        self.native.predict_projected(
            features,
            |scores| {
                let month = local_month()
                    .filter(|month| (1..=12).contains(month))
                    .ok_or(RainHurdleWindInferenceError::InvalidLocalMonth)?;
                project_occurrence(scores, projection, month);
                Ok(month)
            },
            |raw, selected, month| project_selected(raw, selected, projection, month),
        )
    }
}

/// Apply a reviewed occurrence-head projection before category selection.
fn project_occurrence(scores: &mut [f64; 3], projection: RainHurdleWindProjectionId, month: u8) {
    let seasonal = match projection {
        RainHurdleWindProjectionId::R3SpringWetLogitPlus020 => (3..=5).contains(&month),
        RainHurdleWindProjectionId::R4SummerWetLogitPlus020 => (6..=8).contains(&month),
        _ => false,
    };
    // adjust only the selected seasonal wet head with float32 output semantics
    if seasonal {
        let probability = js_min(1.0 - 1e-12, js_max(1e-12, scores[0]));
        let logit = (probability / (1.0 - probability)).ln() + 0.20;
        scores[0] = f64::from((1.0 / (1.0 + (-logit).exp())) as f32);
    }
    // enforce cumulative nesting only for its independent projection arm
    if projection == RainHurdleWindProjectionId::R5NestedCumulativeMin {
        for index in 1..scores.len() {
            scores[index] = js_min(scores[index - 1], scores[index]);
        }
    }
}

/// Apply the reviewed post-category projection and overriding raw wet guard.
fn project_selected(
    raw: f64,
    selected: f64,
    projection: RainHurdleWindProjectionId,
    month: u8,
) -> f64 {
    let mut projected = selected;
    let winter = matches!(month, 12 | 1 | 2);
    // scale only winter hours in either selected scale arm
    if winter
        && matches!(
            projection,
            RainHurdleWindProjectionId::R1WinterScale090
                | RainHurdleWindProjectionId::R2WinterScale095
        )
    {
        projected *= if projection == RainHurdleWindProjectionId::R1WinterScale090 {
            0.90
        } else {
            0.95
        };
    }
    // return every raw heavy amount, overriding the R6 blend exactly as Node does
    if raw >= 1.0 {
        return raw;
    }
    // retain every lower raw wet call even when no learned category fired
    if raw >= 0.1 {
        return js_max(0.1, projected);
    }
    projected
}

/// Preserve JavaScript Math.min NaN propagation.
fn js_min(left: f64, right: f64) -> f64 {
    // return NaN whenever either operand is not a number
    if left.is_nan() || right.is_nan() {
        f64::NAN
    } else {
        left.min(right)
    }
}

/// Preserve JavaScript Math.max NaN propagation.
fn js_max(left: f64, right: f64) -> f64 {
    // return NaN whenever either operand is not a number
    if left.is_nan() || right.is_nan() {
        f64::NAN
    } else {
        left.max(right)
    }
}
