//! Prepare an exact installed rain shadow artifact for replay by the worker.

use weather_api::admit_rain_portable_artifact;
use weather_contracts::{
    RainHurdleWindFeatureProjection, RainHurdleWindPortableArtifact, RainWindPerformanceResult,
};
use weather_database::Sha256Digest;
use weather_worker::{
    RainAdjustmentEvaluation, RainPortableReplayCalendar, TaskFailure,
    replay_rain_maintenance_candidate, score_rain_control_ordinals,
};

use crate::ApiMaintenanceShadowEvidenceValues;
use crate::maintenance_canonical_json::valid_hash;

/// Numerical material from an independently selected catalog member, not publish authority.
pub struct PreparedWorkerRainMaintenanceCandidate {
    identity: Sha256Digest,
    artifact: RainHurdleWindPortableArtifact,
}

impl PreparedWorkerRainMaintenanceCandidate {
    /// The publication owner still accepts only the original v1 rain runtime artifact.
    pub(crate) fn is_legacy_shadow_artifact(&self) -> bool {
        matches!(&self.artifact, RainHurdleWindPortableArtifact::V1(_))
    }

    /// Borrow the selected artifact month without parsing its bytes again.
    pub(crate) fn model_month(&self) -> &str {
        match &self.artifact {
            RainHurdleWindPortableArtifact::V1(artifact) => &artifact.modelMonth,
            RainHurdleWindPortableArtifact::V2(artifact) => &artifact.modelMonth,
        }
    }

    /// Bind the selected canonical artifact bytes before decoding and checking its model tables.
    pub fn prepare(
        values: &impl ApiMaintenanceShadowEvidenceValues,
        bytes: &[u8],
        bundle_sha256: &str,
    ) -> Option<Self> {
        // require the artifact address supplied by the retained installed receipt
        if !valid_hash(bundle_sha256) {
            return None;
        }
        let identity = values.sha256(bytes)?;
        // refuse byte substitution before decoding the material
        if identity.as_str() != bundle_sha256 {
            return None;
        }
        let doc = values.decode_shadow_json(bytes)?;
        // retain only the canonical sorted artifact selected by the installed catalog
        if values.encode_shadow_sorted_json(&doc)? != bytes {
            return None;
        }
        let artifact = admit_rain_portable_artifact(
            &doc,
            |value| values.decode_shadow_rain_artifact(value),
            |names| values.sha256(&values.encode_shadow_ordered_json(names)?),
        )?;
        Some(Self { identity, artifact })
    }

    /// Score the exact feature rows retained by the incumbent worker without rebuilding them.
    pub fn replay(
        &self,
        incumbent: &RainAdjustmentEvaluation,
        calendar: &mut impl RainPortableReplayCalendar,
    ) -> Result<RainWindPerformanceResult, TaskFailure> {
        replay_rain_maintenance_candidate(
            &incumbent.feature_projection,
            self.identity.as_str(),
            &self.artifact,
            calendar,
        )
    }

    /// Evaluate fixed controls without granting a portable artifact serving authority.
    pub(crate) fn score_control_ordinals(
        &self,
        projection: &RainHurdleWindFeatureProjection,
    ) -> Result<Vec<f64>, TaskFailure> {
        score_rain_control_ordinals(projection, &self.artifact)
    }
}
