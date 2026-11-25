//! Native durable filesystem effects for the two custody-anchor installation commands.

use std::fs::{self, DirBuilder};
use std::io::ErrorKind;
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::path::{Path, PathBuf};

use crate::capture_epoch_operator::read_operator_file;
use crate::custody_anchor_install::CustodyAnchorInstallFiles;
use crate::maintenance_anchor_status::read_private_anchor_slot;
use crate::shadow_metadata_custody_write_files::{
    cleanup_private_temporary, sync_private_directory, write_private_temporary,
};
use crate::{CustodyAnchorKind, MaintenanceAnchorSlot, MaintenanceAnchorStatusError};

const EVIDENCE_ROOT: &str = "/var/lib/weather/xweather/adjustment-evidence";
const WITNESS_PATH: &str = "/opt/weather/current/deploy/state/adjustment-capture-epoch.json";

/// Fixed installed paths and independently sampled file-owner identities.
pub struct NativeCustodyAnchorInstallFiles {
    evidence_root: PathBuf,
    witness_path: PathBuf,
    uid: u32,
    gid: u32,
    witness_uid: u32,
    witness_gid: u32,
}

impl NativeCustodyAnchorInstallFiles {
    /// Bind the installed evidence and root-owned witness paths.
    pub fn native(uid: u32, gid: u32) -> Self {
        Self::for_paths(
            PathBuf::from(EVIDENCE_ROOT),
            PathBuf::from(WITNESS_PATH),
            uid,
            gid,
            0,
            0,
        )
    }

    /// Bind explicitly selected fixture paths without performing an operation.
    pub fn for_paths(
        root: PathBuf,
        witness: PathBuf,
        uid: u32,
        gid: u32,
        witness_uid: u32,
        witness_gid: u32,
    ) -> Self {
        Self {
            evidence_root: root,
            witness_path: witness,
            uid,
            gid,
            witness_uid,
            witness_gid,
        }
    }

    // resolve one fixed kind-owned directory without accepting a path operand
    fn anchor_root(&self, kind: CustodyAnchorKind) -> PathBuf {
        self.evidence_root.join(kind.directory())
    }
}

impl CustodyAnchorInstallFiles for NativeCustodyAnchorInstallFiles {
    /// Borrow the fixed evidence root selected at construction.
    fn evidence_root(&self) -> &Path {
        &self.evidence_root
    }

    /// Read the immutable capture witness through the existing owner-checked boundary.
    fn witness(&mut self) -> Result<Vec<u8>, MaintenanceAnchorStatusError> {
        read_operator_file(&self.witness_path, self.witness_uid, self.witness_gid)
            .map_err(|_| MaintenanceAnchorStatusError::Slot)
    }

    /// Create only the fixed private anchor child beneath an existing private evidence root.
    fn ensure_root(&mut self, kind: CustodyAnchorKind) -> Result<(), MaintenanceAnchorStatusError> {
        require_private_directory(&self.evidence_root, None, None)?;
        let root = self.anchor_root(kind);
        let mut builder = DirBuilder::new();
        // retain an existing path for exact validation rather than repairing it
        match builder.mode(0o700).create(&root) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(_) => return Err(MaintenanceAnchorStatusError::Root),
        }
        require_private_directory(&root, Some(self.uid), Some(self.gid))
    }

    /// Read one exact current, pending, or previous private anchor slot.
    fn read_slot(
        &mut self,
        kind: CustodyAnchorKind,
        slot: MaintenanceAnchorSlot,
    ) -> Result<Option<Vec<u8>>, MaintenanceAnchorStatusError> {
        read_private_anchor_slot(
            &self.anchor_root(kind).join(slot.filename()),
            self.uid,
            self.gid,
        )
    }

    /// Publish the first complete pending anchor exclusively and sync its directory.
    fn create_pending(
        &mut self,
        kind: CustodyAnchorKind,
        bytes: &[u8],
    ) -> Result<(), MaintenanceAnchorStatusError> {
        let root = self.anchor_root(kind);
        let temporary = write_private_temporary(&root, "anchor-pending", bytes)
            .map_err(|_| MaintenanceAnchorStatusError::Slot)?;
        // hard linking preserves the first pending transaction without replacement
        if fs::hard_link(
            &temporary,
            root.join(MaintenanceAnchorSlot::Pending.filename()),
        )
        .is_err()
        {
            cleanup_private_temporary(&temporary)
                .map_err(|_| MaintenanceAnchorStatusError::Slot)?;
            return Err(MaintenanceAnchorStatusError::Slot);
        }
        cleanup_private_temporary(&temporary).map_err(|_| MaintenanceAnchorStatusError::Slot)?;
        sync_private_directory(&root).map_err(|_| MaintenanceAnchorStatusError::Slot)
    }

    /// Atomically replace only the current or direct-predecessor slot.
    fn replace_slot(
        &mut self,
        kind: CustodyAnchorKind,
        slot: MaintenanceAnchorSlot,
        bytes: &[u8],
    ) -> Result<(), MaintenanceAnchorStatusError> {
        // pending publication is exclusive and cannot use the replacement path
        if slot == MaintenanceAnchorSlot::Pending {
            return Err(MaintenanceAnchorStatusError::Slot);
        }
        let root = self.anchor_root(kind);
        let temporary = write_private_temporary(&root, "anchor-replace", bytes)
            .map_err(|_| MaintenanceAnchorStatusError::Slot)?;
        // rename one completely synced transaction inode onto the selected fixed slot
        if fs::rename(&temporary, root.join(slot.filename())).is_err() {
            cleanup_private_temporary(&temporary)
                .map_err(|_| MaintenanceAnchorStatusError::Slot)?;
            return Err(MaintenanceAnchorStatusError::Slot);
        }
        sync_private_directory(&root).map_err(|_| MaintenanceAnchorStatusError::Slot)
    }

    /// Remove only the fixed pending marker after current is durable.
    fn remove_pending(
        &mut self,
        kind: CustodyAnchorKind,
    ) -> Result<(), MaintenanceAnchorStatusError> {
        fs::remove_file(
            self.anchor_root(kind)
                .join(MaintenanceAnchorSlot::Pending.filename()),
        )
        .map_err(|_| MaintenanceAnchorStatusError::Slot)
    }

    /// Flush the existing fixed anchor directory after marker retirement.
    fn sync_root(&mut self, kind: CustodyAnchorKind) -> Result<(), MaintenanceAnchorStatusError> {
        sync_private_directory(&self.anchor_root(kind))
            .map_err(|_| MaintenanceAnchorStatusError::Slot)
    }
}

// require one literal canonical private directory with optional service ownership
fn require_private_directory(
    path: &Path,
    uid: Option<u32>,
    gid: Option<u32>,
) -> Result<(), MaintenanceAnchorStatusError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| MaintenanceAnchorStatusError::Root)?;
    // retain the predecessor's mode, path identity, and child-owner requirements
    if !metadata.is_dir()
        || metadata.mode() & 0o777 != 0o700
        || fs::canonicalize(path).map_err(|_| MaintenanceAnchorStatusError::Root)? != path
        || uid.is_some_and(|value| metadata.uid() != value)
        || gid.is_some_and(|value| metadata.gid() != value)
    {
        return Err(MaintenanceAnchorStatusError::Root);
    }
    Ok(())
}
