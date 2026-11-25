//! Choose the oldest durable final closure not yet checkpointed by the cold archive.

use super::forecast_cycle_final_ack::{
    ForecastCycleFinalAckCodec, ForecastCycleFinalAckError,
    NativeForecastCycleFinalAcknowledgements,
};
use super::forecast_cycle_final_store::{
    ForecastCycleFinalManifestCodec, ForecastCycleFinalManifestError,
    NativeForecastCycleFinalManifests, RetainedForecastCycleManifest,
};

/// Typed failures keep invalid final custody separate from ordinary queue emptiness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeForecastPendingFinalError {
    RootMismatch,
    Manifest(ForecastCycleFinalManifestError),
    Acknowledgement(ForecastCycleFinalAckError),
}

/// Read one exact oldest pending final after validating the original immutable census.
pub fn next_native_forecast_final<ManifestCodec, ManifestStatfs, AckCodec, AckStatfs>(
    manifests: &NativeForecastCycleFinalManifests<ManifestCodec, ManifestStatfs>,
    acknowledgements: &NativeForecastCycleFinalAcknowledgements<AckCodec, AckStatfs>,
) -> Result<
    Option<RetainedForecastCycleManifest<ManifestCodec::Manifest>>,
    NativeForecastPendingFinalError,
>
where
    ManifestCodec: ForecastCycleFinalManifestCodec,
    AckCodec: ForecastCycleFinalAckCodec,
{
    use NativeForecastPendingFinalError as Error;

    // keep both immutable metadata classes in the same admitted archive root
    if manifests.root() != acknowledgements.root() {
        return Err(Error::RootMismatch);
    }
    let mut identities = manifests
        .validated_final_identities()
        .map_err(Error::Manifest)?;
    // preserve source due-key order and retain filename order within an equal due key
    identities.sort_by(|left, right| left.due_key.cmp(&right.due_key));
    // do not read later acknowledgements after finding the source's first truly pending final
    for entry in identities {
        let acknowledged = acknowledgements
            .read_final_acknowledgement(entry.manifest_sha256)
            .map_err(Error::Acknowledgement)?;
        if acknowledged.is_none() {
            return manifests
                .read_final_manifest(entry.manifest_sha256)
                .map_err(Error::Manifest);
        }
    }
    Ok(None)
}
