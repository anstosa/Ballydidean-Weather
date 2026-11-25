//! Concrete immutable workstation archive composition over actual capacity and CAS owners.

use crate::{
    NativeMaintenanceCasMember, NativeMaintenanceCasMemberInventory,
    NativeMaintenanceCasMemberValidator, NativeMaintenanceCasStreamValidator,
    NativeMaintenanceGraphManifest, NativeMaintenancePlaintextObjectAdmission,
    NativeMaintenancePlaintextObjectStore, NativeMaintenancePlaintextObjectStoreError,
    NativeMaintenanceVerifiedFullGraph, encode_native_maintenance_cas_pack,
    measure_native_maintenance_capacity_evidence,
    native_maintenance_retirement_files_v2::format_digest, read_native_maintenance_graph_manifest,
    verify_native_maintenance_full_graph,
};

const FREE_INODE_FLOOR: u64 = 32_768;

/// Actual stored CAS result with source member addresses for graph construction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenancePublishedCas {
    pub created: bool,
    pub object_sha256: [u8; 32],
    pub members: Vec<NativeMaintenanceCasMemberInventory>,
}

/// Closed source archive operation failures without private path diagnostics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenancePlaintextArchiveError {
    Store(NativeMaintenancePlaintextObjectStoreError),
    Capacity,
    Cas,
    Graph,
}

/// Actual fixed-root archive and mandatory domain validation registry.
pub struct NativeMaintenancePlaintextArchive<Validators> {
    store: NativeMaintenancePlaintextObjectStore,
    validators: Validators,
}

impl<Validators> NativeMaintenancePlaintextArchive<Validators>
where
    Validators: NativeMaintenanceCasMemberValidator + NativeMaintenanceCasStreamValidator,
{
    /// Initialize the fixed native ext4 archive directory through the genuine store.
    pub fn initialize(
        validators: Validators,
    ) -> Result<Self, NativeMaintenancePlaintextArchiveError> {
        let store = NativeMaintenancePlaintextObjectStore::initialize()
            .map_err(NativeMaintenancePlaintextArchiveError::Store)?;
        Ok(Self { store, validators })
    }

    /// Publish one validated immutable CAS object after measuring both actual physical layers.
    pub fn publish_cas_object(
        &mut self,
        members: Vec<NativeMaintenanceCasMember<'_>>,
    ) -> Result<NativeMaintenancePublishedCas, NativeMaintenancePlaintextArchiveError> {
        use NativeMaintenancePlaintextArchiveError as Error;
        let bytes =
            encode_native_maintenance_cas_pack(members, &mut self.validators).ok_or(Error::Cas)?;
        let digest = weather_edge::RevisionArchiveCanonicalCodec::sha256(
            &crate::NativeWirePrimitives,
            &bytes,
        );
        let hex = format_digest(digest);
        // reuse only a fully streamed and validated exact object without charging it again
        if let Some(existing) = self
            .store
            .inspect_object(&hex, &mut self.validators)
            .map_err(Error::Store)?
        {
            return Ok(NativeMaintenancePublishedCas {
                created: false,
                object_sha256: digest,
                members: existing.members,
            });
        }
        let measured =
            measure_native_maintenance_capacity_evidence().map_err(|_| Error::Capacity)?;
        let publication = self.store.measure(&measured).map_err(Error::Store)?;
        let committed = self
            .store
            .commit_object(
                &bytes,
                NativeMaintenancePlaintextObjectAdmission {
                    envelope: &publication,
                    free_inode_floor: FREE_INODE_FLOOR,
                },
            )
            .map_err(Error::Store)?;
        let verified = self
            .store
            .inspect_object(&hex, &mut self.validators)
            .map_err(Error::Store)?
            .ok_or(Error::Cas)?;
        // require the final content identity after an actual create or identical concurrent publication
        if committed.object_sha256 != hex || verified.object_sha256 != digest {
            return Err(Error::Cas);
        }
        Ok(NativeMaintenancePublishedCas {
            created: committed.created,
            object_sha256: digest,
            members: verified.members,
        })
    }

    /// Publish the actual canonical graph manifest as the source single-member CAS object.
    pub fn publish_graph_manifest(
        &mut self,
        manifest: &NativeMaintenanceGraphManifest,
    ) -> Result<NativeMaintenancePublishedCas, NativeMaintenancePlaintextArchiveError> {
        let bytes = manifest
            .canonical_bytes()
            .ok_or(NativeMaintenancePlaintextArchiveError::Graph)?;
        // verify the complete source manifest before any archive publication
        if read_native_maintenance_graph_manifest(&bytes).as_ref() != Some(manifest) {
            return Err(NativeMaintenancePlaintextArchiveError::Graph);
        }
        let identity = weather_edge::RevisionArchiveCanonicalCodec::sha256(
            &crate::NativeWirePrimitives,
            &bytes,
        );
        self.publish_cas_object(vec![NativeMaintenanceCasMember {
            kind: "adjustment-graph-manifest/v2",
            identity_sha256: identity,
            payload: &bytes,
        }])
    }

    /// Verify the complete source graph and its optionally retained predecessor chain.
    pub fn verify_full_graph(
        &mut self,
        head: [u8; 32],
        verify_predecessors: bool,
    ) -> Result<NativeMaintenanceVerifiedFullGraph, NativeMaintenancePlaintextArchiveError> {
        verify_native_maintenance_full_graph(
            &mut self.store,
            head,
            &mut self.validators,
            verify_predecessors,
        )
        .ok_or(NativeMaintenancePlaintextArchiveError::Graph)
    }

    /// Rotate the actual hash-only head only after full verified immutable ancestry.
    pub fn update_head(
        &mut self,
        head: [u8; 32],
    ) -> Result<(), NativeMaintenancePlaintextArchiveError> {
        let verified = self.verify_full_graph(head, true)?;
        self.store
            .rotate_verified_head(&verified)
            .map_err(NativeMaintenancePlaintextArchiveError::Store)
    }
}

impl<Validators> crate::NativeMaintenanceArchiveSegmentArchive
    for NativeMaintenancePlaintextArchive<Validators>
where
    Validators: NativeMaintenanceCasMemberValidator + NativeMaintenanceCasStreamValidator,
{
    type Error = NativeMaintenancePlaintextArchiveError;

    /// Read only the actual immutable current graph pointer.
    fn read_head(&mut self) -> Result<Option<[u8; 32]>, Self::Error> {
        self.store.read_head().map_err(Self::Error::Store)
    }

    /// Verify every source-linked immutable graph in the full chain.
    fn verify_full_graph(
        &mut self,
        head: [u8; 32],
    ) -> Result<NativeMaintenanceVerifiedFullGraph, Self::Error> {
        NativeMaintenancePlaintextArchive::verify_full_graph(self, head, true)
    }

    /// Find an exact same-kind checkpoint by verifying each real predecessor graph.
    fn find_member(
        &mut self,
        mut head: Option<[u8; 32]>,
        checkpoint: [u8; 32],
        kind: &str,
    ) -> Result<Option<[u8; 32]>, Self::Error> {
        let mut visited = std::collections::HashSet::new();
        let identity = format_digest(checkpoint);
        // retain the source exact finite predecessor traversal and cross-kind alias refusal
        while let Some(digest) = head {
            if visited.len() >= 16_744 || !visited.insert(digest) {
                return Err(Self::Error::Graph);
            }
            let verified =
                NativeMaintenancePlaintextArchive::verify_full_graph(self, digest, false)?;
            if let Some(entry) = verified
                .manifest
                .entries
                .iter()
                .find(|entry| entry.identity_sha256 == identity)
            {
                if entry.kind != kind {
                    return Err(Self::Error::Graph);
                }
                return Ok(Some(digest));
            }
            head = verified
                .manifest
                .predecessor_graph_sha256
                .as_deref()
                .map(|value| {
                    weather_edge::RevisionGapSha256::from_lower_hex(value)
                        .map(|hash| *hash.as_bytes())
                        .ok_or(Self::Error::Graph)
                })
                .transpose()?;
        }
        Ok(None)
    }

    /// Publish the exact actual segment through admitted measured immutable CAS storage.
    fn publish_cas_object(
        &mut self,
        segment: &crate::NativeMaintenanceArchiveSegment,
    ) -> Result<NativeMaintenancePublishedCas, Self::Error> {
        let members = segment
            .members
            .iter()
            .map(|member| NativeMaintenanceCasMember {
                kind: &member.kind,
                identity_sha256: member.identity_sha256,
                payload: &member.payload,
            })
            .collect();
        NativeMaintenancePlaintextArchive::publish_cas_object(self, members)
    }

    /// Publish only the actual verified native graph manifest.
    fn publish_graph_manifest(
        &mut self,
        manifest: &NativeMaintenanceGraphManifest,
    ) -> Result<[u8; 32], Self::Error> {
        NativeMaintenancePlaintextArchive::publish_graph_manifest(self, manifest)
            .map(|published| published.object_sha256)
    }

    /// Rotate the verified and fully retained head through the source durable pointer owner.
    fn update_head(&mut self, head: [u8; 32]) -> Result<(), Self::Error> {
        NativeMaintenancePlaintextArchive::update_head(self, head)
    }
}
