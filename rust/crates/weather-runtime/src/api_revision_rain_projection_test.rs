//! Route parity for the two original stage-only rain revision documents.

use weather_api::CanonicalInstant;
use weather_contracts::{
    AdjustmentRainGateFeatureProjection, AdjustmentRainGateFeatureProjectionContractVersionLiteral,
    AdjustmentRainGateFeatureProjectionFamilyLiteral,
    AdjustmentRainGateFeatureProjectionLogicalKey,
    AdjustmentRainGateFeatureProjectionProjectionKindLiteral, AdjustmentRevisionProjectionDocument,
    AdjustmentRevisionProjectionKind,
};
use weather_database::Sha256Digest;

use super::{
    ApiMaintenanceRevisionTransportDocument, RevisionRequestShape, project_revision_document,
};
use crate::{ApiMaintenanceReceiptCanonicalValues, MaintenanceReceiptScalar};

const RUN_AT: &str = "2026-11-24T00:00:00.000Z";
const RECEIVED_AT: &str = "2026-11-24T08:01:00.000Z";
const RUN_EPOCH_MS: i64 = 1_795_478_400_000;

struct Values;

impl ApiMaintenanceReceiptCanonicalValues for Values {
    /// Resolve only the two original rain document clocks.
    fn canonical_utc_epoch_milliseconds(&self, value: &str) -> Option<i64> {
        match value {
            RUN_AT => Some(RUN_EPOCH_MS),
            RECEIVED_AT => Some(RUN_EPOCH_MS + 8 * 3_600_000 + 60_000),
            _ => None,
        }
    }

    /// Stage projection must not hash the already decoded document bytes.
    fn sha256(&self, _: &[u8]) -> Option<Sha256Digest> {
        unreachable!("rain feature stage projection rehashed document bytes")
    }

    /// Stage projection deliberately creates no database publication pointer.
    fn unsigned_receipt_sha256(
        &self,
        _: &[(&str, MaintenanceReceiptScalar<'_>)],
    ) -> Option<Sha256Digest> {
        unreachable!("rain feature stage projection derived a publication key")
    }
}

/// Build one decoded feature-v2 document sufficient for route projection.
fn feature_projection() -> AdjustmentRainGateFeatureProjection {
    AdjustmentRainGateFeatureProjection {
        contractVersion: AdjustmentRainGateFeatureProjectionContractVersionLiteral,
        family: AdjustmentRainGateFeatureProjectionFamilyLiteral,
        logicalKey: AdjustmentRainGateFeatureProjectionLogicalKey {
            inputSha256: "1".repeat(64),
            modelSha256: "2".repeat(64),
            runInitializedAt: RUN_AT.to_owned(),
        },
        logicalReceivedAt: RECEIVED_AT.to_owned(),
        projectionKind: AdjustmentRainGateFeatureProjectionProjectionKindLiteral,
        rows: Vec::new(),
        source: Vec::new(),
        storedContentSha256: "3".repeat(64),
    }
}

/// Preserve feature-v2 staging while rejecting both publication route shapes.
#[test]
fn rain_feature_v2_is_stage_only_without_database_pointers() {
    let document = ApiMaintenanceRevisionTransportDocument::Standard(Box::new(
        AdjustmentRevisionProjectionDocument::RainFeature(feature_projection()),
    ));
    let epoch = CanonicalInstant {
        epoch_milliseconds: RUN_EPOCH_MS,
        utc: RUN_AT.to_owned(),
    };

    let evidence =
        project_revision_document(&document, RevisionRequestShape::Stage, &epoch, &Values)
            .expect("feature v2 remains admissible for archive staging");
    assert_eq!(
        evidence.kind,
        AdjustmentRevisionProjectionKind::RainGateInput
    );
    assert!(evidence.pointers.is_empty());

    assert!(
        project_revision_document(&document, RevisionRequestShape::Single, &epoch, &Values,)
            .is_err()
    );
    assert!(
        project_revision_document(&document, RevisionRequestShape::Grouped, &epoch, &Values,)
            .is_err()
    );
}
