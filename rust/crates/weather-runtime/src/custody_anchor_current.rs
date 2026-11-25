//! Current-only reader for the development and rain-control custody anchor stores.

use std::path::PathBuf;

use weather_api::CurrentTimeResolver;
use weather_contracts::JsonValue;

use crate::custody_anchor_validation::validate_custody_anchor;
use crate::maintenance_anchor_status::{
    MAXIMUM_ANCHOR_BYTES, read_private_anchor_slot, verified_private_anchor_root,
};
use crate::maintenance_canonical_json::valid_hash;
use crate::{MaintenanceAnchorStatusError, MaintenanceAnchorStatusJson};

const EVIDENCE_ROOT: &str = "/var/lib/weather/xweather/adjustment-evidence";

/// Disjoint predecessor custody directories with no terminal-action authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CustodyAnchorKind {
    Development,
    RainControl,
}

impl CustodyAnchorKind {
    // select only one fixed custody directory independently of command operands
    pub(crate) fn directory(self) -> &'static str {
        match self {
            Self::Development => "development-custody-anchors",
            Self::RainControl => "rain-control-custody-anchors",
        }
    }
}

/// The original current-only storage observation.
pub trait CustodyAnchorCurrentFiles {
    /// Read one bounded current anchor or return genuine absence.
    fn read_current(
        &mut self,
        kind: CustodyAnchorKind,
    ) -> Result<Option<Vec<u8>>, MaintenanceAnchorStatusError>;
}

/// Native evidence owner preserving the disjoint fixed roots.
pub struct NativeCustodyAnchorCurrentFiles {
    root: PathBuf,
    uid: u32,
    gid: u32,
}

impl NativeCustodyAnchorCurrentFiles {
    /// Bind the installed evidence path to the real service identity.
    pub fn native(uid: u32, gid: u32) -> Self {
        Self::for_evidence_root(PathBuf::from(EVIDENCE_ROOT), uid, gid)
    }

    /// Bind an already selected evidence root without any operation.
    pub fn for_evidence_root(root: PathBuf, uid: u32, gid: u32) -> Self {
        Self { root, uid, gid }
    }
}

impl CustodyAnchorCurrentFiles for NativeCustodyAnchorCurrentFiles {
    /// Read only current.json under the existing canonical private root.
    fn read_current(
        &mut self,
        kind: CustodyAnchorKind,
    ) -> Result<Option<Vec<u8>>, MaintenanceAnchorStatusError> {
        let root = self.root.join(kind.directory());
        // absence and pending-only stores return the original null without enumeration or recovery
        if !verified_private_anchor_root(&root, self.uid, self.gid)? {
            return Ok(None);
        }
        read_private_anchor_slot(&root.join("current.json"), self.uid, self.gid)
    }
}

/// Execute either original current query through the existing canonical codec.
pub fn read_custody_anchor_current_command(
    action: &str,
    args: &[&str],
    files: &mut impl CustodyAnchorCurrentFiles,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Result<Option<Vec<u8>>, MaintenanceAnchorStatusError> {
    use MaintenanceAnchorStatusError::{
        Codec, DevelopmentCustodyArguments, RainControlCustodyArguments, Slot,
    };
    // close the original zero-operand commands before selecting an evidence root
    let kind = match action {
        "development-custody-anchor-current-v1" if args.is_empty() => {
            CustodyAnchorKind::Development
        }
        "rain-control-custody-anchor-current-v1" if args.is_empty() => {
            CustodyAnchorKind::RainControl
        }
        "development-custody-anchor-current-v1" => return Err(DevelopmentCustodyArguments),
        "rain-control-custody-anchor-current-v1" => return Err(RainControlCustodyArguments),
        _ => return Ok(None),
    };
    // read and bind only the exact current anchor bytes to the returned identity
    let value = match files.read_current(kind)? {
        Some(bytes) => {
            // retain the original canonical document limit and entire schema validation
            if !(2..=MAXIMUM_ANCHOR_BYTES).contains(&bytes.len()) {
                return Err(Slot);
            }
            let anchor = codec.parse_canonical_object(&bytes).ok_or(Slot)?;
            validate_custody_anchor(kind, &anchor, codec, time).ok_or(Slot)?;
            // preserve the exact bytes already held by the predecessor
            if codec.encode_canonical_json(&anchor).as_deref() != Some(bytes.as_slice()) {
                return Err(Slot);
            }
            let digest = codec.sha256(&bytes).ok_or(Codec)?;
            // preserve the typed canonical address contract on the response
            if !valid_hash(digest.as_str()) {
                return Err(Codec);
            }
            JsonValue::Object(vec![
                ("anchor".encode_utf16().collect(), anchor),
                (
                    "anchorSha256".encode_utf16().collect(),
                    JsonValue::String(digest.as_str().encode_utf16().collect()),
                ),
            ])
        }
        None => JsonValue::Null,
    };
    Ok(Some(codec.encode_canonical_json(&value).ok_or(Codec)?))
}
