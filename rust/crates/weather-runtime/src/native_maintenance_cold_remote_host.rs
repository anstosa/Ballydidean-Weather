//! Bind the fixed revision catalog operations to the concrete workstation transport.

use std::{future::Future, io, pin::Pin};

use crate::{
    NativeMaintenanceDatabaseLedgerReader, NativeWirePrimitives,
    capture_epoch_witness::resolve_operator_capture_clock,
    maintenance_canonical_json::valid_hash,
    native_maintenance_cold_remote_documents::{
        verify_native_revision_cold_custody_acknowledgement_v2_document,
        verify_native_revision_cold_start_document,
    },
    native_maintenance_cold_remote_page::verify_native_maintenance_cold_remote_page,
    native_maintenance_revision_cold_drain::{
        NativeRevisionColdCustodyRequest, NativeRevisionColdPageRequest,
        NativeRevisionColdRemotePort, VerifiedNativeRevisionColdCustodyAcknowledgementV2,
        VerifiedNativeRevisionColdPage, VerifiedNativeRevisionColdStart,
    },
};

impl NativeRevisionColdRemotePort for NativeMaintenanceDatabaseLedgerReader {
    type Error = io::Error;

    /// Obtain and verify one exact cutoff-bound source start.
    fn fetch_verified_start<'a>(
        &'a mut self,
        cutoff_at: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<VerifiedNativeRevisionColdStart, Self::Error>> + 'a>>
    {
        Box::pin(async move {
            // admit the source's ordinary canonical cutoff operand
            resolve_operator_capture_clock(cutoff_at, &NativeWirePrimitives).ok_or_else(invalid)?;
            let bytes =
                self.read_document("adjustment-revision-catalog-current-start-v1", &[cutoff_at])?;
            let value = verify_native_revision_cold_start_document(
                &bytes,
                &mut NativeWirePrimitives,
                &NativeWirePrimitives,
            )
            .map_err(|_| invalid())?;
            // bind the validated snapshot to the request's original cutoff
            if value.cutoff_at != cutoff_at {
                return Err(invalid());
            }
            Ok(value)
        })
    }

    /// Request one bounded direct successor and verify all six coordinates.
    fn fetch_verified_page<'a>(
        &'a mut self,
        request: &'a NativeRevisionColdPageRequest,
    ) -> Pin<Box<dyn Future<Output = Result<VerifiedNativeRevisionColdPage, Self::Error>> + 'a>>
    {
        Box::pin(async move {
            let args = page_arguments(request).ok_or_else(invalid)?;
            let bytes = self.read_document("adjustment-revision-catalog-page-v1", &args)?;
            verify_native_maintenance_cold_remote_page(&bytes, request).ok_or_else(invalid)
        })
    }

    /// Acknowledge the exact previously persisted custody request.
    fn acknowledge_verified_v2<'a>(
        &'a mut self,
        request: &'a NativeRevisionColdCustodyRequest,
    ) -> Pin<
        Box<
            dyn Future<
                    Output = Result<
                        VerifiedNativeRevisionColdCustodyAcknowledgementV2,
                        Self::Error,
                    >,
                > + 'a,
        >,
    > {
        Box::pin(async move {
            let mut args = page_arguments(&request.page).ok_or_else(invalid)?.to_vec();
            let identities = [
                &request.page_sha256,
                &request.custody_checkpoint_sha256,
                &request.member_root_sha256,
                &request.start_member_sha256,
            ];
            // retain all four archive-backed source digest operands
            if identities.iter().any(|digest| !valid_hash(digest)) {
                return Err(invalid());
            }
            args.extend(identities.iter().map(|digest| digest.as_str()));
            let bytes = self.read_document("adjustment-revision-custody-ack-v2", &args)?;
            verify_native_revision_cold_custody_acknowledgement_v2_document(
                &bytes,
                request,
                &mut NativeWirePrimitives,
                &NativeWirePrimitives,
            )
            .map_err(|_| invalid())
        })
    }
}

// emit only the fixed six typed catalog coordinates in their source order
fn page_arguments(request: &NativeRevisionColdPageRequest) -> Option<[&str; 6]> {
    // require canonical unsigned ordinals and lowercase hashes before dispatch
    for ordinal in [
        &request.after_archive_commit_ordinal,
        &request.watermark_archive_commit_ordinal,
    ] {
        if ordinal.parse::<u64>().ok()?.to_string() != *ordinal {
            return None;
        }
    }
    for digest in [
        &request.watermark_frontier_sha256,
        &request.start_sha256,
        &request.after_frontier_sha256,
        &request.previous_page_sha256,
    ] {
        if !valid_hash(digest) {
            return None;
        }
    }
    Some([
        &request.watermark_archive_commit_ordinal,
        &request.watermark_frontier_sha256,
        &request.start_sha256,
        &request.after_archive_commit_ordinal,
        &request.after_frontier_sha256,
        &request.previous_page_sha256,
    ])
}

// return bounded source failure without including private payloads
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "revision catalog response unavailable",
    )
}
