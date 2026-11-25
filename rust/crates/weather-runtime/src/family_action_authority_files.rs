//! Native read-only custody for the fixed family-action authority slots.

use std::fs;
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use crate::api_maintenance_candidate_files::open_read_no_follow;
use crate::family_action_authority::{FamilyActionAnchorKind, FamilyActionAuthorityFiles};
use crate::maintenance_anchor_status::{
    MAXIMUM_ANCHOR_BYTES, MaintenanceAnchorStatusError, read_private_anchor_slot,
    verified_private_anchor_root,
};

const EVIDENCE_ROOT: &str = "/var/lib/weather/xweather/adjustment-evidence";

/// Filesystem owner for the host observation and service-owned authority reads.
pub struct NativeFamilyActionAuthorityFiles {
    root: PathBuf,
    uid: u32,
    gid: u32,
    host_uid: u32,
}

impl NativeFamilyActionAuthorityFiles {
    /// Bind the installed evidence root and independently sampled service identity.
    pub fn native(uid: u32, gid: u32) -> Self {
        Self::for_evidence_root(PathBuf::from(EVIDENCE_ROOT), uid, gid, 0)
    }

    /// Bind a selected evidence root and the distinct service and host owners.
    pub fn for_evidence_root(root: PathBuf, uid: u32, gid: u32, host_uid: u32) -> Self {
        Self {
            root,
            uid,
            gid,
            host_uid,
        }
    }

    // select one fixed authority root independently of action contents
    fn current_path(&self, kind: &FamilyActionAnchorKind) -> PathBuf {
        let directory = match kind {
            FamilyActionAnchorKind::Development => "development-custody-anchors",
            FamilyActionAnchorKind::RainControl => "rain-control-custody-anchors",
            FamilyActionAnchorKind::Maintenance => "maintenance-anchors",
        };
        self.root.join(directory).join("current.json")
    }
}

impl FamilyActionAuthorityFiles for NativeFamilyActionAuthorityFiles {
    /// Read the exact host-owned current slot before service authorization.
    fn read_host_current(
        &mut self,
        kind: FamilyActionAnchorKind,
    ) -> Result<Vec<u8>, MaintenanceAnchorStatusError> {
        read_host_file(&self.current_path(&kind), self.host_uid)
    }

    /// Read the service-owned current slot through its store-specific boundary.
    fn read_validated_current(
        &mut self,
        kind: FamilyActionAnchorKind,
    ) -> Result<Option<Vec<u8>>, MaintenanceAnchorStatusError> {
        let path = self.current_path(&kind);
        // custody stores validate their private root before observing current
        if matches!(
            &kind,
            FamilyActionAnchorKind::Development | FamilyActionAnchorKind::RainControl
        ) && !verified_private_anchor_root(
            path.parent().ok_or(MaintenanceAnchorStatusError::Root)?,
            self.uid,
            self.gid,
        )? {
            return Ok(None);
        }
        // maintenance authorization reads its exact slot without root inventory
        read_private_anchor_slot(&path, self.uid, self.gid)
    }
}

// reproduce the bounded host-owned descriptor read without following the final link
fn read_host_file(path: &Path, host_uid: u32) -> Result<Vec<u8>, MaintenanceAnchorStatusError> {
    use MaintenanceAnchorStatusError::Slot;

    let mut file = open_read_no_follow(path).map_err(|_| Slot)?;
    let before = file.metadata().map_err(|_| Slot)?;
    let size = usize::try_from(before.size()).map_err(|_| Slot)?;
    // require one canonical root-owned private file before reading any bytes
    if !before.is_file()
        || before.nlink() != 1
        || before.uid() != host_uid
        || before.gid() != before.uid()
        || before.mode() & 0o777 != 0o600
        || !(1..=MAXIMUM_ANCHOR_BYTES).contains(&size)
        || fs::canonicalize(path).map_err(|_| Slot)? != path
    {
        return Err(Slot);
    }
    let mut bytes = Vec::with_capacity(size);
    (&mut file)
        .take(MAXIMUM_ANCHOR_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Slot)?;
    let after = file.metadata().map_err(|_| Slot)?;
    // reject a short read, growth, replacement or nanosecond mutation during the read
    if bytes.len() != size
        || before.dev() != after.dev()
        || before.ino() != after.ino()
        || before.size() != after.size()
        || before.mtime() != after.mtime()
        || before.mtime_nsec() != after.mtime_nsec()
    {
        return Err(Slot);
    }
    Ok(bytes)
}
