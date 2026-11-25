//! Fixed scorecard publication transport and exact installed-root acknowledgement projection.

use std::io;

use weather_contracts::JsonValue;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};

use crate::{
    FORECAST_SCORECARD_V2, NativeMaintenanceDatabaseLedgerReader,
    NativeMaintenanceScorecardPublication, NativeWirePrimitives,
    maintenance_canonical_json::{text, valid_hash},
    native_maintenance_retirement_files_v2::format_digest,
    parse_forecast_adjustment_scorecard,
};

const INSTALL_OPERATION: &str = "install-adjustment-scorecard-v2";

/// Closed fixed transport for the sole v2 scorecard installation operation.
pub trait NativeMaintenanceScorecardPublicationTransport {
    /// Submit one canonical scorecard under its exact immutable operand.
    fn install_scorecard_v2(
        &mut self,
        scorecard_sha256: &str,
        canonical_scorecard: &[u8],
    ) -> io::Result<()>;
}

impl NativeMaintenanceScorecardPublicationTransport for NativeMaintenanceDatabaseLedgerReader {
    /// Reuse the installed release's fixed forced-command SSH owner.
    fn install_scorecard_v2(
        &mut self,
        scorecard_sha256: &str,
        canonical_scorecard: &[u8],
    ) -> io::Result<()> {
        self.request_document(
            INSTALL_OPERATION,
            &[scorecard_sha256],
            Some(canonical_scorecard),
        )
        .map(|_| ())
    }
}

/// The exact public root returned after one verified installation acknowledgement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceScorecardPublicationRoot {
    pub scorecard_sha256: String,
}

impl NativeMaintenanceScorecardPublicationRoot {
    /// Project the predecessor's one-field successful publication result.
    pub fn to_json(&self) -> JsonValue {
        JsonValue::Object(vec![(
            "scorecardSha256".encode_utf16().collect(),
            JsonValue::String(self.scorecard_sha256.encode_utf16().collect()),
        )])
    }
}

/// Closed local material, clock, transport, and acknowledgement failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceScorecardTransportError {
    Material,
    Clock,
    Transport,
}

/// Revalidate, publish, and release only an exactly acknowledged scorecard root.
pub fn publish_native_maintenance_scorecard_v2(
    publication: &NativeMaintenanceScorecardPublication,
    now: &str,
    transport: &mut impl NativeMaintenanceScorecardPublicationTransport,
) -> Result<NativeMaintenanceScorecardPublicationRoot, NativeMaintenanceScorecardTransportError> {
    use NativeMaintenanceScorecardTransportError as Error;

    let wire = NativeWirePrimitives;
    let now_ms = wire.canonical_utc_milliseconds(now).ok_or(Error::Clock)?;
    let decoded = parse_forecast_adjustment_scorecard(
        &publication.bytes,
        Some(now_ms),
        &mut NativeWirePrimitives,
    )
    .map_err(|_| Error::Material)?;
    let canonical = wire.encode_sorted_json(&decoded).ok_or(Error::Material)?;
    let actual_sha256 = format_digest(wire.sha256(&publication.bytes));
    // bind the public value, exact canonical bytes, v2 generation, and immutable operand
    if wire.encode_sorted_json(&publication.scorecard).as_deref()
        != Some(publication.bytes.as_slice())
        || canonical != publication.bytes
        || text(&decoded, "contractVersion").as_deref() != Some(FORECAST_SCORECARD_V2)
        || !valid_hash(&publication.scorecard_sha256)
        || actual_sha256 != publication.scorecard_sha256
    {
        return Err(Error::Material);
    }
    transport
        .install_scorecard_v2(&publication.scorecard_sha256, &publication.bytes)
        .map_err(|_| Error::Transport)?;
    Ok(NativeMaintenanceScorecardPublicationRoot {
        scorecard_sha256: publication.scorecard_sha256.clone(),
    })
}

#[cfg(test)]
#[path = "native_maintenance_scorecard_transport_test.rs"]
mod tests;
