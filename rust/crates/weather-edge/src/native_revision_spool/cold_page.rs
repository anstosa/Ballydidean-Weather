//! Execute the original cold page command against the native private spool.

use std::io::Write;

use super::{
    NativeRevisionColdEntryCodec, NativeRevisionColdGroupCodec, NativeRevisionSpool,
    RevisionSpoolLeaseCodec, require_directory,
};
use crate::revision_archive_canonical::RevisionArchiveCanonicalCodec;
use crate::revision_cold_page::{
    RevisionColdCursor, RevisionColdPageEntry, RevisionColdPageError, RevisionColdPageRequest,
    compose_revision_cold_page, parse_revision_cold_page_command,
};

const COLD_DIRECTORIES: [&str; 6] = [
    "revision-commit-receipts",
    "revision-frontier-successors",
    "revision-projections",
    "shadow-revision-capsules",
    "rain-control-states",
    "rain-control-state-stage-receipts",
];

/// The original command's canonical archive codecs.
pub trait NativeRevisionColdPageCodec:
    NativeRevisionColdEntryCodec + NativeRevisionColdGroupCodec + RevisionArchiveCanonicalCodec
{
}

impl<Codec> NativeRevisionColdPageCodec for Codec where
    Codec:
        NativeRevisionColdEntryCodec + NativeRevisionColdGroupCodec + RevisionArchiveCanonicalCodec
{
}

impl<Lease: RevisionSpoolLeaseCodec> NativeRevisionSpool<Lease> {
    /// Read a canonical two-slot successor page without creating or mutating files.
    pub fn read_revision_cold_page(
        &self,
        request: RevisionColdPageRequest,
        codec: &impl NativeRevisionColdPageCodec,
    ) -> Result<Vec<u8>, RevisionColdPageError> {
        use RevisionColdPageError as Error;
        compose_revision_cold_page(
            request,
            codec,
            || {
                require_directory(&self.root).map_err(|_| Error::Storage)?;
                // validate the fixed literal children before looking for a successor
                for directory in COLD_DIRECTORIES {
                    require_directory(&self.root.join(directory)).map_err(|_| Error::Storage)?;
                }
                Ok(())
            },
            |after, watermark| {
                let group = self
                    .read_revision_cold_group(after.frontier, codec)
                    .map_err(|_| Error::Successor)?;
                let first = group.receipts.first().ok_or(Error::Successor)?;
                let last = group.receipts.last().ok_or(Error::Successor)?;
                // reject a group outside the selected watermark before reading payloads
                if first.predecessor_frontier_sha256 != after.frontier
                    || first.archive_commit_ordinal <= after.ordinal
                    || last.archive_commit_ordinal > watermark.ordinal
                {
                    return Err(Error::Successor);
                }
                let chain = group
                    .receipts
                    .iter()
                    .map(
                        // preserve every complete group's original receipt coordinates
                        |receipt| {
                            (
                                receipt.predecessor_frontier_sha256,
                                RevisionColdCursor {
                                    ordinal: receipt.archive_commit_ordinal,
                                    frontier: receipt.frontier_sha256,
                                },
                            )
                        },
                    )
                    .collect();
                let canonical = self
                    .read_revision_cold_entry(&group, codec)
                    .map_err(|_| Error::Payload)?;
                Ok(RevisionColdPageEntry { chain, canonical })
            },
        )
    }

    /// Run the original command and write exactly one complete canonical response.
    pub fn run_revision_cold_page_command(
        &self,
        action: &str,
        operands: &[&str],
        codec: &impl NativeRevisionColdPageCodec,
        output: &mut impl Write,
    ) -> Result<bool, RevisionColdPageError> {
        let Some(request) = parse_revision_cold_page_command(action, operands)? else {
            return Ok(false);
        };
        let bytes = self.read_revision_cold_page(request, codec)?;
        output
            .write_all(&bytes)
            .map_err(|_| RevisionColdPageError::Storage)?;
        Ok(true)
    }
}
