//! Shared read-only legacy archive metadata authority without creating storage directories.

use std::path::PathBuf;

use weather_edge::{
    ArchiveWireCodec, ArchiveWirePrimitives, ForecastCycleFinalAckCodec,
    ForecastCycleFinalAcknowledgement, ForecastCycleFinalManifest, ForecastCycleFinalManifestCodec,
    ForecastCycleTransferCheckpointCodec, RetainedForecastCycleManifest,
};

use crate::{
    MaintenanceCanonicalJson, NativeMaintenanceAnchorInstallV2Error,
    NativeMaintenanceAnchorV2Transfer, NativeMaintenanceAnchorV2TransferAuthority,
    NativeMaintenanceRetirementExpectedV2, NativeMaintenanceRetirementExpectedV2Authority,
    NativeMaintenanceRetirementExpectedV2Error, NativeMaintenanceRetirementKindV2,
    maintenance_canonical_json::text,
    native_maintenance_anchor_install_v2::parse_digest,
    native_maintenance_retirement_expected_v2::{
        AdmittedNativeMaintenanceRetirementFinalV2,
        collect_native_maintenance_retirement_entries_v2,
    },
    native_maintenance_retirement_files_v2::{format_digest, read_expected_maintenance_member_v2},
};

/// Read the bounded retained cycle metadata through one already admitted exact metadata codec.
pub struct NativeMaintenanceLegacyMetadataV2<Archive, Canonical, Primitives> {
    root: PathBuf,
    archive: Archive,
    canonical: Canonical,
    wire: ArchiveWireCodec<Primitives>,
}

impl<Archive, Canonical, Primitives>
    NativeMaintenanceLegacyMetadataV2<Archive, Canonical, Primitives>
{
    /// Bind the selected source evidence root and reusable verified metadata owners.
    pub const fn new(
        root: PathBuf,
        archive: Archive,
        canonical: Canonical,
        wire: ArchiveWireCodec<Primitives>,
    ) -> Self {
        Self {
            root,
            archive,
            canonical,
            wire,
        }
    }
}

impl<Archive, Canonical, Primitives>
    NativeMaintenanceLegacyMetadataV2<Archive, Canonical, Primitives>
where
    Archive: ForecastCycleFinalManifestCodec<Manifest = ForecastCycleFinalManifest>
        + ForecastCycleFinalAckCodec,
{
    /// Join only the exact canonical retained final and acknowledgement addresses.
    fn read_final_transfer(
        &self,
        manifest_sha256: &str,
    ) -> Result<
        Option<(
            RetainedForecastCycleManifest<ForecastCycleFinalManifest>,
            ForecastCycleFinalAcknowledgement,
        )>,
        NativeMaintenanceRetirementExpectedV2Error,
    > {
        use NativeMaintenanceRetirementExpectedV2Error as Error;
        let digest = parse_digest(manifest_sha256).ok_or(Error::FinalManifest)?;
        let Some(bytes) = read_expected_maintenance_member_v2(
            &self.root,
            NativeMaintenanceRetirementKindV2::FinalManifest,
            manifest_sha256,
        )
        .map_err(|_| Error::Store)?
        else {
            return Ok(None);
        };
        // bind complete exact original bytes to the requested immutable manifest name
        if self.archive.sha256(&bytes) != *digest.as_bytes() {
            return Err(Error::FinalManifest);
        }
        let manifest = self
            .archive
            .decode_final_manifest(&bytes)
            .ok_or(Error::FinalManifest)?;
        let Some(ack_bytes) = read_expected_maintenance_member_v2(
            &self.root,
            NativeMaintenanceRetirementKindV2::FinalAcknowledgement,
            manifest_sha256,
        )
        .map_err(|_| Error::Store)?
        else {
            return Ok(None);
        };
        let acknowledgement = self
            .archive
            .decode_final_acknowledgement(&ack_bytes)
            .ok_or(Error::FinalAcknowledgement)?;
        // bind the cold-graph acknowledgement to the same canonical manifest identity
        if acknowledgement.manifest_sha256 != digest {
            return Err(Error::FinalAcknowledgement);
        }
        Ok(Some((
            RetainedForecastCycleManifest {
                manifest,
                manifest_sha256: digest,
            },
            acknowledgement,
        )))
    }
}

impl<Archive, Canonical, Primitives> NativeMaintenanceAnchorV2TransferAuthority
    for NativeMaintenanceLegacyMetadataV2<Archive, Canonical, Primitives>
where
    Archive: ForecastCycleFinalManifestCodec<Manifest = ForecastCycleFinalManifest>
        + ForecastCycleFinalAckCodec,
{
    /// Project the exact legacy T authority from its retained canonical transfer records.
    fn read_transfer(
        &mut self,
        manifest_sha256: &str,
    ) -> Result<Option<NativeMaintenanceAnchorV2Transfer>, NativeMaintenanceAnchorInstallV2Error>
    {
        use NativeMaintenanceAnchorInstallV2Error as Error;
        let Some((manifest, acknowledgement)) = self
            .read_final_transfer(manifest_sha256)
            .map_err(|_| Error::Transfer)?
        else {
            return Ok(None);
        };
        let document = manifest.manifest.document();
        Ok(Some(NativeMaintenanceAnchorV2Transfer {
            due_key: text(document, "dueKey").ok_or(Error::Transfer)?,
            generation: text(document, "generation").ok_or(Error::Transfer)?,
            graph_sha256: format_digest(*acknowledgement.graph_sha256.as_bytes()),
            acknowledged_at: acknowledgement.acknowledged_at,
        }))
    }
}

impl<Archive, Canonical, Primitives> NativeMaintenanceRetirementExpectedV2Authority
    for NativeMaintenanceLegacyMetadataV2<Archive, Canonical, Primitives>
where
    Archive: ForecastCycleFinalManifestCodec<Manifest = ForecastCycleFinalManifest>
        + ForecastCycleFinalAckCodec
        + ForecastCycleTransferCheckpointCodec,
    Canonical: MaintenanceCanonicalJson,
    Primitives: ArchiveWirePrimitives,
{
    /// Reconstruct the exact legacy graph from the actual immutable final and its retained members.
    fn read_expected(
        &mut self,
        manifest_sha256: &str,
    ) -> Result<NativeMaintenanceRetirementExpectedV2, NativeMaintenanceRetirementExpectedV2Error>
    {
        use NativeMaintenanceRetirementExpectedV2Error as Error;
        let (manifest, acknowledgement) = self
            .read_final_transfer(manifest_sha256)?
            .ok_or(Error::FinalManifest)?;
        let admitted = AdmittedNativeMaintenanceRetirementFinalV2::from_retained(&manifest);
        collect_native_maintenance_retirement_entries_v2(
            &self.root,
            &admitted,
            &acknowledgement,
            &mut self.canonical,
            &self.wire,
            &self.archive,
        )
    }
}

#[cfg(test)]
#[path = "native_maintenance_legacy_metadata_v2_test.rs"]
mod tests;
